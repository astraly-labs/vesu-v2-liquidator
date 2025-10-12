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

        self.0.get(&ONCHAIN_ASSETS[ticker]).map(|t| *t).expect(
            "Every ticker in our Vesu Prices must have a price. See `wait_for_first_prices`.",
        )
    }

    /// Wait until the first prices are populated.
    pub async fn wait_for_first_prices(&self) {
        const CHECK_INTERVAL: Duration = Duration::from_secs(2);

        loop {
            if self.0.iter().all(|t| !t.is_zero()) {
                return;
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    }
}
