use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use dashmap::DashMap;
use rust_decimal::Decimal;

use crate::{
    config::onchain_assets::{ONCHAIN_ASSETS, OnchainAssetConfig},
    types::currency::Currency,
};

pub static VESU_PRICES: LazyLock<Arc<VesuOraclePrices>> =
    LazyLock::new(|| Arc::new(VesuOraclePrices::new()));

/// Map contaning the price in dollars for a list of monitored assets.
#[derive(Default, Debug, Clone)]
pub struct VesuOraclePrices(pub DashMap<OnchainAssetConfig, Decimal>);

impl VesuOraclePrices {
    pub fn new() -> Self {
        let prices = DashMap::new();
        for asset in &ONCHAIN_ASSETS.all() {
            prices.insert(asset.clone(), Decimal::ZERO);
        }
        Self(prices)
    }

    pub fn of(&self, currency: Currency) -> Decimal {
        self.of_ticker(currency.as_ref())
    }

    pub fn of_ticker(&self, ticker: &str) -> Decimal {
        if ticker.eq_ignore_ascii_case("USD") {
            return Decimal::ONE;
        }

        self.0
            .get(&ONCHAIN_ASSETS[ticker])
            .map(|price| *price)
            .unwrap_or_else(|| panic!("{ticker} is missing from config/assets.toml"))
    }

    /// Waits until every asset has a price, giving up after `timeout`.
    ///
    /// The Vesu oracle does not necessarily price every asset listed in
    /// `assets.toml`, and waiting for all of them unconditionally stalls the caller
    /// forever. Positions using an unpriced asset are skipped by
    /// `VesuPosition::is_liquidable` instead.
    pub async fn wait_for_first_prices(&self, timeout: Duration) {
        const CHECK_INTERVAL: Duration = Duration::from_secs(2);

        let all_priced = async {
            loop {
                if self.0.iter().all(|t| !t.is_zero()) {
                    return;
                }
                tokio::time::sleep(CHECK_INTERVAL).await;
            }
        };

        if tokio::time::timeout(timeout, all_priced).await.is_err() {
            let unpriced: Vec<String> = self
                .0
                .iter()
                .filter(|entry| entry.is_zero())
                .map(|entry| entry.key().ticker.clone())
                .collect();
            tracing::warn!(
                "[🔮 Oracle] No Vesu price for {unpriced:?} after {timeout:?}; positions using them will be skipped"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Vesu oracle does not price every configured asset, so this must return
    /// instead of stalling the monitoring service forever.
    #[tokio::test]
    async fn wait_for_first_prices_gives_up_on_unpriced_assets() {
        let prices = VesuOraclePrices::new();

        tokio::time::timeout(
            Duration::from_secs(10),
            prices.wait_for_first_prices(Duration::from_millis(50)),
        )
        .await
        .expect("must not wait for a price that never comes");
    }
}
