use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use serde::{Deserialize, Serialize};
use starknet::core::types::Felt;

use crate::types::currency::Currency;

// Global static available from anywhere - makes it easier to have the information
// about any on chain asset at any point in the code.
// This structure is guaranteed to be constant and never change through the code
// runtime.
pub static ONCHAIN_ASSETS: LazyLock<Arc<OnchainAssets>> =
    LazyLock::new(|| Arc::new(OnchainAssets::new()));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct OnchainAssetConfig {
    pub name: String,
    pub ticker: String,
    pub decimals: u32,
    #[serde(deserialize_with = "deserialize_felt_from_str")]
    pub address: Felt,
}

/// Represents the assets.toml configuration file
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssetsConfig {
    pub assets: Vec<OnchainAssetConfig>,
}

impl AssetsConfig {
    pub fn new() -> Self {
        const CONFIG_CONTENT: &str = include_str!("../../config/assets.toml");
        toml::from_str(CONFIG_CONTENT).expect("Failed to parse assets.toml")
    }
}

#[derive(Debug, Clone)]
pub struct OnchainAssets {
    by_ticker: HashMap<String, OnchainAssetConfig>,
    by_address: HashMap<Felt, OnchainAssetConfig>,
    assets: Vec<OnchainAssetConfig>,
}

impl OnchainAssets {
    pub fn new() -> Self {
        let assets_config = AssetsConfig::new();

        let mut by_ticker = HashMap::new();
        let mut by_address = HashMap::new();

        for asset in &assets_config.assets {
            by_ticker.insert(asset.ticker.clone(), asset.clone());
            by_address.insert(asset.address, asset.clone());
        }

        Self {
            by_ticker,
            by_address,
            assets: assets_config.assets,
        }
    }

    pub fn get_by_ticker(&self, ticker: &str) -> Option<&OnchainAssetConfig> {
        self.by_ticker.get(ticker)
    }

    pub fn get_by_address(&self, address: &Felt) -> Option<&OnchainAssetConfig> {
        self.by_address.get(address)
    }

    pub fn all(&self) -> Vec<OnchainAssetConfig> {
        self.assets.clone()
    }
}

impl Default for OnchainAssets {
    fn default() -> Self {
        Self::new()
    }
}

/// Allows us to retrieve a config by ticker, cf: config[Currency::ETH]
impl std::ops::Index<Currency> for OnchainAssets {
    type Output = OnchainAssetConfig;

    fn index(&self, currency: Currency) -> &Self::Output {
        self.by_ticker
            .get(&currency.to_string())
            .unwrap_or_else(|| panic!("Asset for currency {currency} not found"))
    }
}

/// Allows us to retrieve a config by ticker, cf: config["USDC"]
impl std::ops::Index<&str> for OnchainAssets {
    type Output = OnchainAssetConfig;

    fn index(&self, ticker: &str) -> &Self::Output {
        self.by_ticker
            .get(ticker)
            .unwrap_or_else(|| panic!("Asset with ticker '{ticker}' not found"))
    }
}

/// Allows us to retrieve a config by a starknet address, cf: config[&FELT]
impl std::ops::Index<&Felt> for OnchainAssets {
    type Output = OnchainAssetConfig;

    fn index(&self, address: &Felt) -> &Self::Output {
        self.by_address
            .get(address)
            .unwrap_or_else(|| panic!("Asset with starknet address '{address:#x}' not found"))
    }
}

// Custom deserializer to convert strings to Felt for addresses
fn deserialize_felt_from_str<'de, D>(deserializer: D) -> Result<Felt, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Felt::from_hex(&s).map_err(serde::de::Error::custom)
}
