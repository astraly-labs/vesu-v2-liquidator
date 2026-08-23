use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result};
use backon::{ExponentialBuilder, Retryable};
use cainome::cairo_serde::{ContractAddress, U256};
use num_traits::Pow;
use rust_decimal::Decimal;
use serde_json::Value;
use starknet_rust::core::types::Felt;

use crate::bindings::liquidate::{I129, PoolKey, RouteNode, Swap, TokenAmount};
use crate::utils::{HTTP_REQUEST_TIMEOUT, http_client};

const EKUBO_QUOTE_ENDPOINT: &str = "https://quoter-mainnet-api.ekubo.org";
const SCALE: u128 = 1_000_000_000_000_000_000;

/// One shared client: a per-call `reqwest::Client` would leak a connection pool
/// each time, and the default client has no timeout at all.
static EKUBO_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    http_client(HTTP_REQUEST_TIMEOUT).expect("Could not build the Ekubo HTTP client")
});

/// One HTTP round trip to the Ekubo quoter, returning the raw body.
async fn fetch_quote(endpoint: &str) -> Result<String> {
    let response = EKUBO_CLIENT.get(endpoint).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Ekubo quote failed with status {}", response.status());
    }

    Ok(response.text().await?)
}

/// Reads one split's `amount_specified`.
fn parse_amount(split: &Value) -> Result<i128> {
    split["amount_specified"]
        .as_str()
        .context("amount_specified is not a string")?
        .parse::<i128>()
        .context("amount_specified is not an i128")
}

/// This split's share of `total`, in `SCALE` units.
///
/// `amount * SCALE` overflows a `u128` past ~340 units of an 18-decimal token, and
/// release builds wrap silently instead of panicking. Only the ratio matters, so
/// both sides are halved until the multiplication fits.
fn weight_of(amount: i128, total: i128) -> Result<u128> {
    const MAX_NUMERATOR: u128 = u128::MAX / SCALE;

    let mut numerator = amount.unsigned_abs();
    let mut denominator = total.unsigned_abs();

    while numerator > MAX_NUMERATOR {
        numerator >>= 1;
        denominator >>= 1;
    }

    // Also catches a zero total, and a denominator shifted away by the loop.
    anyhow::ensure!(denominator != 0, "Ekubo split total is zero");

    Ok(numerator * SCALE / denominator)
}

/// Fetches a swap route from Ekubo.
///
/// Only the HTTP fetch is retried: it is an idempotent read. Parsing is
/// deterministic, and the liquidation transaction itself must never be replayed
/// automatically.
pub async fn get_ekubo_route(
    from_token: Felt,
    to_token: Felt,
    amount: &Decimal,
    decimals: Decimal,
) -> Result<(Vec<Swap>, Vec<u128>)> {
    let amount = amount * Decimal::TEN.pow(decimals);

    let amount: u128 = amount
        .try_into()
        .with_context(|| format!("swap amount {amount} does not fit in a u128"))?;

    let endpoint = format!(
        "{EKUBO_QUOTE_ENDPOINT}/-{amount}/{}/{}",
        from_token.to_fixed_hex_string(),
        to_token.to_fixed_hex_string()
    );

    let response_text = (|| fetch_quote(&endpoint))
        .retry(
            ExponentialBuilder::default()
                .with_min_delay(Duration::from_millis(200))
                .with_max_delay(Duration::from_secs(2))
                .with_max_times(3)
                .with_jitter(),
        )
        .notify(|e, delay| {
            tracing::warn!("[🔭 Monitoring] Ekubo quote failed ({e}), retrying in {delay:?}");
        })
        .await?;

    let json_value: Value = serde_json::from_str(&response_text)?;

    let splits = json_value["splits"]
        .as_array()
        .context("'splits' is not an array")?;

    if splits.is_empty() {
        anyhow::bail!("No splits returned from Ekubo API");
    }

    // Handle single split case (100% weight)
    if splits.len() == 1 {
        let route = parse_route(&splits[0])?;
        return Ok((
            vec![Swap {
                route,
                token_amount: TokenAmount {
                    token: ContractAddress(from_token),
                    amount: I129 {
                        mag: 0,
                        sign: false,
                    },
                },
            }],
            vec![SCALE], // Single weight of 100%
        ));
    }

    let mut total_amount: i128 = 0;
    for split in splits {
        total_amount += parse_amount(split)?;
    }
    anyhow::ensure!(
        total_amount != 0,
        "Ekubo returned splits summing to a zero amount"
    );

    let mut swaps = Vec::with_capacity(splits.len());
    let mut weights = Vec::with_capacity(splits.len());
    let mut running_weight_sum: u128 = 0;

    // Process all splits except the last one
    for split in splits.iter().take(splits.len() - 1) {
        let split_amount = parse_amount(split)?;

        let weight = weight_of(split_amount, total_amount)?;
        running_weight_sum += weight;
        weights.push(weight);

        let route = parse_route(split)?;
        swaps.push(Swap {
            route,
            token_amount: TokenAmount {
                token: ContractAddress(from_token),
                amount: I129 {
                    mag: 0,
                    sign: false,
                },
            },
        });
    }

    // Handle the last split - ensure exact SCALE total
    let last_split = splits.last().unwrap();
    let last_weight = SCALE - running_weight_sum;
    weights.push(last_weight);

    let route = parse_route(last_split)?;
    swaps.push(Swap {
        route,
        token_amount: TokenAmount {
            token: ContractAddress(from_token),
            amount: I129 {
                mag: 0,
                sign: false,
            },
        },
    });

    // Verify total is exactly SCALE
    let total_weight: u128 = weights.iter().sum();
    assert!(total_weight == SCALE, "Weights do not sum to SCALE");

    Ok((swaps, weights))
}

fn parse_route(split: &Value) -> Result<Vec<RouteNode>> {
    split["route"]
        .as_array()
        .context("'route' is not an array")?
        .iter()
        .map(|node| {
            let pool_key = &node["pool_key"];
            let sqrt_ratio_limit = node["sqrt_ratio_limit"]
                .as_str()
                .context("sqrt_ratio_limit is not a string")?;

            let sqrt_ratio = U256::from_bytes_be(&Felt::from_hex(sqrt_ratio_limit)?.to_bytes_be());

            Ok(RouteNode {
                pool_key: PoolKey {
                    token0: ContractAddress(Felt::from_hex(
                        pool_key["token0"]
                            .as_str()
                            .context("token0 is not a string")?,
                    )?),
                    token1: ContractAddress(Felt::from_hex(
                        pool_key["token1"]
                            .as_str()
                            .context("token1 is not a string")?,
                    )?),
                    fee: u128::from_str_radix(
                        pool_key["fee"]
                            .as_str()
                            .context("fee is not a string")?
                            .trim_start_matches("0x"),
                        16,
                    )
                    .context("Failed to parse fee as u128")?,
                    tick_spacing: pool_key["tick_spacing"]
                        .as_u64()
                        .context("tick_spacing is not a u64")?
                        as u128,
                    extension: ContractAddress(Felt::from_hex(
                        pool_key["extension"]
                            .as_str()
                            .context("extension is not a string")?,
                    )?),
                },
                sqrt_ratio_limit: sqrt_ratio,
                skip_ahead: node["skip_ahead"]
                    .as_u64()
                    .context("skip_ahead is not a u64")? as u128,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 50/50 split of a large 18-decimal debt must stay 50/50. `amount * SCALE`
    /// overflows a `u128` past ~340 units, and a release build wraps silently: the
    /// swap would then be routed at a ratio unrelated to the quote, and the
    /// `total_weight == SCALE` assertion cannot catch it.
    #[test]
    fn split_weights_survive_a_large_18_decimal_amount() {
        let half = 500 * 10_i128.pow(18);
        let total = 2 * half;

        assert_eq!(weight_of(half, total).unwrap(), SCALE / 2);
        assert_eq!(weight_of(total, total).unwrap(), SCALE);
    }

    #[test]
    fn weight_of_rejects_a_zero_total() {
        assert!(weight_of(0, 0).is_err());
    }
}
