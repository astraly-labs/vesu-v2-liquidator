use rust_decimal::Decimal;

use crate::{config::onchain_assets::ONCHAIN_ASSETS, services::oracle::vesu_prices::VESU_PRICES};

#[allow(non_camel_case_types)]
#[derive(
    Debug,
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    strum::Display,
    strum::AsRefStr,
    strum::EnumString,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[strum(ascii_case_insensitive)]
pub enum Currency {
    USDC,
    #[strum(serialize = "USDC.E")]
    USDC_E,
    USDT,
    STRK,
    xSTRK,
    ETH,
    wstETH,
    WBTC,
    tBTC,
    uniBTC,
    solvBTC,
    LBTC,
    xsBTC,
    xWBTC,
    xtBTC,
    mRe7BTC,
    xLBTC,
    #[strum(serialize = "YBTC.B")]
    YBTC_B,
    mRe7YIELD,
    sUSN,
    strkBTC,
    xstrkBTC,
    EKUBO,
}

impl Currency {
    pub fn decimals(&self) -> u32 {
        ONCHAIN_ASSETS[*self].decimals
    }

    pub fn d_decimals(&self) -> Decimal {
        Decimal::from(self.decimals())
    }

    pub fn address(&self) -> starknet_rust::core::types::Felt {
        ONCHAIN_ASSETS[*self].address
    }

    pub fn price(&self) -> Decimal {
        VESU_PRICES.of(*self)
    }
}
