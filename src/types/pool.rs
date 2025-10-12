use anyhow::bail;
use evian::vesu::v2::data::indexer::events::{
    CollateralAddress, DebtAddress, PoolAddress, PoolDetails,
};
use serde::{Deserialize, Serialize};
use starknet::{core::types::Felt, macros::felt_hex};

use crate::types::currency::Currency;

pub type VesuPoolId = Felt;

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    strum::EnumString,
    strum::Display,
    strum::EnumIter,
    Hash,
)]
pub enum PoolName {
    Prime,
    Re7USDCPrime,
    Re7USDCCore,
    Re7xBTC,
    Re7USDCStableCore,
    Re7USDCFrontier,
}

impl PoolName {
    pub const fn pool_address(&self) -> VesuPoolId {
        match self {
            Self::Prime => {
                felt_hex!("0x451fe483d5921a2919ddd81d0de6696669bccdacd859f72a4fba7656b97c3b5")
            }
            Self::Re7USDCPrime => {
                felt_hex!("0x02eef0c13b10b487ea5916b54c0a7f98ec43fb3048f60fdeedaf5b08f6f88aaf")
            }
            Self::Re7USDCCore => {
                felt_hex!("0x03976cac265a12609934089004df458ea29c776d77da423c96dc761d09d24124")
            }
            Self::Re7xBTC => {
                felt_hex!("0x03a8416bf20d036df5b1cf3447630a2e1cb04685f6b0c3a70ed7fb1473548ecf")
            }
            Self::Re7USDCStableCore => {
                felt_hex!("0x073702fce24aba36da1eac539bd4bae62d4d6a76747b7cdd3e016da754d7a135")
            }
            Self::Re7USDCFrontier => {
                felt_hex!("0x05c03e7e0ccfe79c634782388eb1e6ed4e8e2a013ab0fcc055140805e46261bd")
            }
        }
    }

    pub fn pool_details(&self, collateral: Currency, debt: Currency) -> PoolDetails {
        PoolDetails {
            pool_address: PoolAddress(self.pool_address()),
            collateral_address: CollateralAddress(collateral.address()),
            debt_address: DebtAddress(debt.address()),
        }
    }
}

impl TryFrom<&Felt> for PoolName {
    type Error = anyhow::Error;

    fn try_from(value: &Felt) -> Result<Self, Self::Error> {
        match () {
            _ if value == &Self::Prime.pool_address() => Ok(Self::Prime),
            _ if value == &Self::Re7USDCPrime.pool_address() => Ok(Self::Re7USDCPrime),
            _ if value == &Self::Re7USDCCore.pool_address() => Ok(Self::Re7USDCCore),
            _ if value == &Self::Re7xBTC.pool_address() => Ok(Self::Re7xBTC),
            _ if value == &Self::Re7USDCStableCore.pool_address() => Ok(Self::Re7USDCStableCore),
            _ if value == &Self::Re7USDCFrontier.pool_address() => Ok(Self::Re7USDCFrontier),
            () => bail!("Unknown VesuPool for address {value:x}"),
        }
    }
}
