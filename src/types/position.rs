use std::hash::Hash;
use std::str::FromStr;
use std::sync::Arc;

use cainome::cairo_serde::U256;
use colored::Colorize;
use evian::utils::starknet_indexer::handler::StarknetEventMetadata;
use evian::vesu_v2::data::VesuDataClient;
use num_traits::Pow;
use pragma_common::starknet::fallback_provider::FallbackProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use serde::Serialize;
use starknet_rust::core::types::Call;
use starknet_rust::core::types::Felt;

use crate::bindings::liquidate::Liquidate;
use crate::bindings::liquidate::LiquidateParams;
use crate::config::onchain_assets::ONCHAIN_ASSETS;
use crate::services::indexer::PositionDelta;
use crate::services::monitoring::ekubo::get_ekubo_route;
use crate::types::account::StarknetSingleOwnerAccount;
use crate::types::currency::Currency;
use crate::types::pool::PoolName;

const VESU_SCALE: Decimal = dec!(18);

/// Identity of a Vesu position: a pool, an asset pair and a user.
///
/// Vesu v2 keys positions by exactly this tuple. Deriving `Hash` replaces the two
/// hand-rolled hashes this code used to compute — one from the raw indexer event,
/// one from the built position — which agreed only by accident.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PositionKey {
    pub pool: PoolName,
    pub collateral: Felt,
    pub debt: Felt,
    pub user: Felt,
}

impl PositionKey {
    /// Builds the key straight from an indexer event.
    pub fn from_delta(pool: PoolName, delta: &PositionDelta) -> Self {
        Self {
            pool,
            collateral: delta.collateral_address,
            debt: delta.debt_address,
            user: delta.user_address,
        }
    }
}

#[derive(Clone, Hash, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct VesuPosition {
    pub user_address: Felt,
    pub pool_name: PoolName,
    pub collateral: Asset,
    pub debt: Asset,
    pub lltv: Decimal,
}

impl VesuPosition {
    /// Creates a new Position from a Vesu Event.
    pub async fn new(
        event_metadata: &StarknetEventMetadata,
        vesu_client: &Arc<VesuDataClient<FallbackProvider>>,
        event: PositionDelta,
    ) -> anyhow::Result<Self> {
        let mut new_position = Self {
            user_address: event.user_address,
            pool_name: PoolName::try_from(&event_metadata.from_address).expect(
                "Unsupported pool. Should never happen if we indexed the pools using `PoolName`",
            ),
            collateral: Asset::from_address(event.collateral_address),
            debt: Asset::from_address(event.debt_address),
            lltv: Decimal::ZERO,
        };

        new_position.update_lltv(vesu_client).await?;
        anyhow::ensure!(!new_position.lltv.is_zero(), "LLTV cannot be zero.");

        new_position.update_from_delta(event);

        Ok(new_position)
    }

    /// Given a new delta event, update the position.
    pub fn update_from_delta(&mut self, delta: PositionDelta) {
        let collateral_delta = scale(delta.collateral_delta, VESU_SCALE);
        self.collateral.apply_delta(collateral_delta);
        let debt_delta = scale(delta.debt_delta, VESU_SCALE);
        self.debt.apply_delta(debt_delta);
    }

    /// Updates the LLTV of the position.
    async fn update_lltv(
        &mut self,
        vesu_client: &Arc<VesuDataClient<FallbackProvider>>,
    ) -> anyhow::Result<()> {
        let pair_config = vesu_client
            .pair_config(
                self.pool_name.pool_address(),
                self.collateral.address,
                self.debt.address,
                None,
            )
            .await?;

        self.lltv = pair_config.max_ltv;

        if pair_config.max_ltv.is_zero() {
            tracing::warn!(
                "For {} {}-{} ; max LTV is zero...?",
                self.pool_name,
                self.collateral.currency,
                self.debt.currency,
            );
        }

        Ok(())
    }

    /// Check if the current position is closed.
    pub fn is_closed(&self) -> bool {
        self.collateral.amount.is_zero() || self.collateral.amount.is_sign_negative()
    }

    /// Returns the identity of this position.
    pub fn key(&self) -> PositionKey {
        PositionKey {
            pool: self.pool_name,
            collateral: self.collateral.address,
            debt: self.debt.address,
            user: self.user_address,
        }
    }

    /// Returns the collateral value in usd.
    pub fn collateral_value_in_usd(&self) -> Decimal {
        let collateral_price = self.collateral.currency.price();
        self.collateral.amount * collateral_price
    }

    /// Returns the debt value in usd.
    pub fn debt_value_in_usd(&self) -> Decimal {
        let debt_price = self.debt.currency.price();
        self.debt.amount * debt_price
    }

    /// Returns the current LTV, or `None` when the collateral is worth nothing.
    ///
    /// `Decimal` division panics on a zero divisor, so the guard belongs here and
    /// not in the callers.
    pub fn ltv(&self) -> Option<Decimal> {
        let collateral_value = self.collateral_value_in_usd();
        (!collateral_value.is_zero()).then(|| self.debt_value_in_usd() / collateral_value)
    }

    /// Check if the current position is liquidable.
    /// Also logs a warning if the position is close to being liquidable.
    pub fn is_liquidable(&self) -> bool {
        const ALMOST_LIQUIDABLE_THRESHOLD: Decimal = dec!(0.1);

        if self.lltv.is_zero() {
            return false;
        }

        // A missing price makes the LTV meaningless. Never send a liquidation on it.
        if self.collateral.currency.price().is_zero() || self.debt.currency.price().is_zero() {
            tracing::warn!(
                "[🔭 Monitoring] No {}/{} price, skipping {self}",
                self.collateral.currency,
                self.debt.currency,
            );
            return false;
        }

        // `None` here means the collateral amount is zero: nothing left to seize.
        let Some(ltv_ratio) = self.ltv() else {
            return false;
        };

        let is_liquidable = ltv_ratio >= self.lltv;
        let almost_liquidable_threshold = self.lltv - ALMOST_LIQUIDABLE_THRESHOLD;
        let is_almost_liquidable = !is_liquidable && ltv_ratio > almost_liquidable_threshold;

        if is_liquidable || is_almost_liquidable {
            self.logs_liquidation_state(is_liquidable, ltv_ratio);
        }

        is_liquidable
    }

    fn logs_liquidation_state(&self, is_liquidable: bool, ltv_ratio: Decimal) {
        tracing::info!(
            "{} is at ratio {:.2}%/{:.2}% => {}",
            self,
            ltv_ratio * dec!(100),
            self.lltv * dec!(100),
            if is_liquidable {
                "liquidable! 🚨".green()
            } else {
                "almost liquidable 🔫".yellow()
            }
        );
    }

    /// Returns the TX necessary to liquidate this position using the Vesu Liquidate
    /// contract.
    pub async fn get_vesu_liquidate_tx(
        &self,
        liquidate_contract: &Arc<Liquidate<StarknetSingleOwnerAccount>>,
        liquidator_address: &Felt,
    ) -> anyhow::Result<Call> {
        let (liquidate_swap, liquidate_swap_weights) = get_ekubo_route(
            self.debt.address,
            self.collateral.address,
            &self.debt.amount,
            self.debt.decimals,
        )
        .await?;

        let liquidate_params = LiquidateParams {
            pool: cainome::cairo_serde::ContractAddress(self.pool_name.pool_address()),
            collateral_asset: cainome::cairo_serde::ContractAddress(self.collateral.address),
            debt_asset: cainome::cairo_serde::ContractAddress(self.debt.address),
            user: cainome::cairo_serde::ContractAddress(self.user_address),
            recipient: cainome::cairo_serde::ContractAddress(*liquidator_address),
            min_collateral_to_receive: U256 { low: 0, high: 0 },
            debt_to_repay: U256 { low: 0, high: 0 },
            liquidate_swap,
            liquidate_swap_weights,
            liquidate_swap_limit_amount: u128::MAX,
            withdraw_swap: vec![],
            withdraw_swap_limit_amount: 0,
            withdraw_swap_weights: vec![],
        };

        Ok(liquidate_contract.liquidate_getcall(&liquidate_params))
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub currency: Currency,
    pub address: Felt,
    pub decimals: Decimal,
    pub amount: Decimal,
}

impl Asset {
    /// Builds an asset from its on-chain address, which must be listed in
    /// `config/assets.toml`.
    ///
    /// The address is kept verbatim: resolving it back through the ticker would
    /// make `PositionKey` depend on that round trip staying an identity.
    pub fn from_address(address: Felt) -> Self {
        let config = &ONCHAIN_ASSETS[&address];

        let currency =
            Currency::from_str(&config.ticker).expect("Could not convert ticker -> Currency");

        Self {
            name: config.name.clone(),
            decimals: currency.d_decimals(),
            address,
            currency,
            amount: Decimal::ZERO,
        }
    }

    pub fn apply_delta(&mut self, amount_delta: Decimal) {
        self.amount += amount_delta;
    }
}

fn scale(nb: Decimal, scale: Decimal) -> Decimal {
    nb / Decimal::TEN.pow(scale)
}

impl std::fmt::Display for VesuPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Position of {:#x} on {} with {} {} of collateral and {} {} of debt",
            self.user_address,
            self.pool_name,
            self.collateral
                .amount
                .round_dp(self.collateral.decimals.try_into().expect("Must fit")),
            self.collateral.currency,
            self.debt
                .amount
                .round_dp(self.debt.decimals.try_into().expect("Must fit")),
            self.debt.currency,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unpriced asset must neither panic (`Decimal` division panics on a zero
    /// divisor) nor be reported as liquidable on data we do not have.
    #[test]
    fn is_liquidable_skips_an_unpriced_position() {
        // `VESU_PRICES` holds zeros until the oracle service fills it in.
        let mut position = VesuPosition {
            user_address: Felt::ONE,
            pool_name: PoolName::Prime,
            collateral: Asset::from_address(Currency::ETH.address()),
            debt: Asset::from_address(Currency::USDC.address()),
            lltv: dec!(0.8),
        };
        position.collateral.amount = dec!(1);
        position.debt.amount = dec!(100);

        assert_eq!(position.ltv(), None);
        assert!(!position.is_liquidable());
    }

    /// `PositionKey` is built from raw event addresses on one side and from
    /// `Asset::address` on the other. They must be the same value.
    #[test]
    fn asset_keeps_the_address_it_was_built_from() {
        for asset in &ONCHAIN_ASSETS.all() {
            assert_eq!(
                Asset::from_address(asset.address).address,
                asset.address,
                "{} was rewritten while being resolved",
                asset.ticker
            );
        }
    }
}
