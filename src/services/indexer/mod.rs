pub mod task;

use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use evian::{
    utils::indexer::handler::{OutputEvent, StarknetEventMetadata},
    vesu::v2::data::{
        VesuDataClient,
        indexer::{
            VesuDataIndexer,
            events::{LiquidatePositionEvent, PoolDetails, PositionEvent, VesuEvent},
        },
    },
};
use pragma_common::starknet::{StarknetNetwork, fallback_provider::FallbackProvider};
use rust_decimal::Decimal;
use starknet::core::types::Felt;
use tokio::sync::{mpsc, oneshot};

use crate::types::{currency::Currency, pool::PoolName};

pub struct IndexerService {
    pub current_block: u64,
    pub apibara_api_key: String,
    pub provider: FallbackProvider,
    pub tx_to_monitoring: mpsc::UnboundedSender<(StarknetEventMetadata, PositionDelta)>,
    meet_with_monitoring: Option<oneshot::Sender<()>>,
}

#[derive(Debug, Clone)]
pub struct PositionDelta {
    pub collateral_address: Felt,
    pub debt_address: Felt,
    pub user_address: Felt,
    pub collateral_delta: Decimal,
    pub debt_delta: Decimal,
}

impl IndexerService {
    pub fn new(
        starting_block: u64,
        apibara_api_key: String,
        provider: FallbackProvider,
        tx_to_monitoring: mpsc::UnboundedSender<(StarknetEventMetadata, PositionDelta)>,
        meet_with_monitoring: oneshot::Sender<()>,
    ) -> Self {
        Self {
            current_block: starting_block,
            apibara_api_key,
            provider,
            tx_to_monitoring,
            meet_with_monitoring: Some(meet_with_monitoring),
        }
    }

    async fn run_forever(&mut self) -> anyhow::Result<()> {
        let vesu_indexer = self.initialize_indexer().await?;

        let (mut rx_messages, mut vesu_handle) = vesu_indexer.start(None).await?;

        tracing::info!(
            "[🔢 Indexer] 🔌 Connected to Vesu! (from block {})",
            self.current_block
        );

        loop {
            tokio::select! {
                Some(msg) = rx_messages.recv() => {
                    match msg {
                        OutputEvent::Event { event_metadata, event } => {
                            match event {
                                VesuEvent::Position(position) => {
                                    self.current_block = event_metadata.block_number + 1;
                                    self.tx_to_monitoring.send((event_metadata, position.into()))?;
                                },
                                VesuEvent::Liquidation(liquidation) => {
                                    self.current_block = event_metadata.block_number + 1;
                                    self.tx_to_monitoring.send((event_metadata, liquidation.into()))?;
                                }
                                VesuEvent::Context(_) => {
                                }
                            }

                        }
                        OutputEvent::Synced => {
                            tracing::info!("[🔢 Indexer] 🥳 Vesu indexer reached the tip of the chain!");

                            if let Some(meet_with_monitoring) = self.meet_with_monitoring.take() {
                                meet_with_monitoring.send(()).expect("Rendezvous from Indexer dropped?");
                            }
                        }
                        // TODO: Handle re-orgs.
                        OutputEvent::Finalized(_) | OutputEvent::Invalidated(_) => { }
                    }
                }

                res = &mut vesu_handle => {
                    anyhow::bail!("😱 Vesu indexer stopped: {res:?}");
                }
            }
        }
    }

    /// Initialize the Vesu indexer.
    async fn initialize_indexer(&self) -> Result<VesuDataIndexer<FallbackProvider>> {
        let vesu_client = Arc::new(VesuDataClient::new(
            StarknetNetwork::Mainnet,
            self.provider.clone(),
        ));

        let vesu_indexer = VesuDataIndexer::new(
            vesu_client,
            self.apibara_api_key.clone(),
            Self::monitored_pools(),
            None,
            self.current_block,
        )?;

        Ok(vesu_indexer)
    }

    /// Returns all the v2 pools monitored by the liquidation bot.
    /// Source: https://vesu.xyz/borrow
    fn monitored_pools() -> HashSet<PoolDetails> {
        [
            PoolName::Re7USDCCore.pool_details(Currency::uniBTC, Currency::USDC),
            PoolName::Re7USDCCore.pool_details(Currency::LBTC, Currency::USDC),
            PoolName::Re7USDCCore.pool_details(Currency::tBTC, Currency::USDC),
            PoolName::Re7USDCCore.pool_details(Currency::solvBTC, Currency::USDC),
            PoolName::Re7USDCCore.pool_details(Currency::xWBTC, Currency::USDC),
            PoolName::Re7USDCCore.pool_details(Currency::xLBTC, Currency::USDC),
            PoolName::Re7USDCCore.pool_details(Currency::xsBTC, Currency::USDC),
            PoolName::Re7USDCCore.pool_details(Currency::xtBTC, Currency::USDC),
            PoolName::Re7USDCCore.pool_details(Currency::WBTC, Currency::USDC),
            PoolName::Re7USDCPrime.pool_details(Currency::WBTC, Currency::USDC),
            PoolName::Re7xBTC.pool_details(Currency::xtBTC, Currency::solvBTC),
            PoolName::Re7xBTC.pool_details(Currency::mRe7BTC, Currency::solvBTC),
            PoolName::Re7xBTC.pool_details(Currency::xsBTC, Currency::solvBTC),
            PoolName::Re7xBTC.pool_details(Currency::xWBTC, Currency::solvBTC),
            PoolName::Re7xBTC.pool_details(Currency::xLBTC, Currency::solvBTC),
            PoolName::Re7xBTC.pool_details(Currency::xtBTC, Currency::tBTC),
            PoolName::Re7xBTC.pool_details(Currency::mRe7BTC, Currency::tBTC),
            PoolName::Re7xBTC.pool_details(Currency::xsBTC, Currency::tBTC),
            PoolName::Re7xBTC.pool_details(Currency::xWBTC, Currency::tBTC),
            PoolName::Re7xBTC.pool_details(Currency::xLBTC, Currency::tBTC),
            PoolName::Re7xBTC.pool_details(Currency::xtBTC, Currency::LBTC),
            PoolName::Re7xBTC.pool_details(Currency::mRe7BTC, Currency::LBTC),
            PoolName::Re7xBTC.pool_details(Currency::xsBTC, Currency::LBTC),
            PoolName::Re7xBTC.pool_details(Currency::xWBTC, Currency::LBTC),
            PoolName::Re7xBTC.pool_details(Currency::xtBTC, Currency::WBTC),
            PoolName::Re7xBTC.pool_details(Currency::mRe7BTC, Currency::WBTC),
            PoolName::Re7xBTC.pool_details(Currency::xsBTC, Currency::WBTC),
            PoolName::Re7xBTC.pool_details(Currency::xWBTC, Currency::WBTC),
            PoolName::Re7xBTC.pool_details(Currency::xLBTC, Currency::WBTC),
            PoolName::Re7xBTC.pool_details(Currency::xLBTC, Currency::LBTC),
            PoolName::Re7USDCFrontier.pool_details(Currency::YBTC_B, Currency::USDC),
            PoolName::Re7USDCStableCore.pool_details(Currency::mRe7YIELD, Currency::USDC),
            PoolName::Re7USDCStableCore.pool_details(Currency::sUSN, Currency::USDC),
            PoolName::Prime.pool_details(Currency::wstETH, Currency::ETH),
            PoolName::Prime.pool_details(Currency::WBTC, Currency::ETH),
            PoolName::Prime.pool_details(Currency::STRK, Currency::ETH),
            PoolName::Prime.pool_details(Currency::USDC, Currency::ETH),
            PoolName::Prime.pool_details(Currency::USDT, Currency::ETH),
            PoolName::Prime.pool_details(Currency::wstETH, Currency::STRK),
            PoolName::Prime.pool_details(Currency::WBTC, Currency::STRK),
            PoolName::Prime.pool_details(Currency::ETH, Currency::STRK),
            PoolName::Prime.pool_details(Currency::USDC, Currency::STRK),
            PoolName::Prime.pool_details(Currency::USDT, Currency::STRK),
            PoolName::Prime.pool_details(Currency::wstETH, Currency::USDC),
            PoolName::Prime.pool_details(Currency::WBTC, Currency::USDC),
            PoolName::Prime.pool_details(Currency::STRK, Currency::USDC),
            PoolName::Prime.pool_details(Currency::ETH, Currency::USDC),
            PoolName::Prime.pool_details(Currency::USDT, Currency::USDC),
            PoolName::Prime.pool_details(Currency::wstETH, Currency::USDT),
            PoolName::Prime.pool_details(Currency::WBTC, Currency::USDT),
            PoolName::Prime.pool_details(Currency::STRK, Currency::USDT),
            PoolName::Prime.pool_details(Currency::ETH, Currency::USDT),
            PoolName::Prime.pool_details(Currency::USDC, Currency::USDT),
            PoolName::Prime.pool_details(Currency::wstETH, Currency::WBTC),
            PoolName::Prime.pool_details(Currency::STRK, Currency::WBTC),
            PoolName::Prime.pool_details(Currency::ETH, Currency::WBTC),
            PoolName::Prime.pool_details(Currency::USDC, Currency::WBTC),
            PoolName::Prime.pool_details(Currency::USDT, Currency::WBTC),
            PoolName::Prime.pool_details(Currency::WBTC, Currency::wstETH),
            PoolName::Prime.pool_details(Currency::STRK, Currency::wstETH),
            PoolName::Prime.pool_details(Currency::ETH, Currency::wstETH),
            PoolName::Prime.pool_details(Currency::USDC, Currency::wstETH),
            PoolName::Prime.pool_details(Currency::USDT, Currency::wstETH),
            PoolName::Prime.pool_details(Currency::xSTRK, Currency::USDC),
            PoolName::Prime.pool_details(Currency::xSTRK, Currency::STRK),
            PoolName::Prime.pool_details(Currency::xSTRK, Currency::USDT),
            PoolName::Prime.pool_details(Currency::xWBTC, Currency::USDC),
            PoolName::Prime.pool_details(Currency::xWBTC, Currency::WBTC),
            PoolName::Prime.pool_details(Currency::xWBTC, Currency::USDT),
        ]
        .into()
    }
}

impl From<PositionEvent> for PositionDelta {
    fn from(value: PositionEvent) -> Self {
        Self {
            collateral_address: value.event_metadata.collateral_address,
            debt_address: value.event_metadata.debt_address,
            user_address: value.event_metadata.user_address.0,
            collateral_delta: value.collateral_delta,
            debt_delta: value.debt_delta,
        }
    }
}

impl From<LiquidatePositionEvent> for PositionDelta {
    fn from(value: LiquidatePositionEvent) -> Self {
        Self {
            collateral_address: value.event_metadata.collateral_address,
            debt_address: value.event_metadata.debt_address,
            user_address: value.event_metadata.user_address.0,
            collateral_delta: value.collateral_delta,
            debt_delta: value.debt_delta,
        }
    }
}
