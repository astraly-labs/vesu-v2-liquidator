pub mod ekubo;
pub mod task;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use evian::{utils::indexer::handler::StarknetEventMetadata, vesu::v2::data::VesuDataClient};
use pragma_common::starknet::{FallbackProvider, StarknetNetwork};
use starknet::core::types::Felt;
use starknet::macros::felt_hex;
use tokio::sync::{mpsc, oneshot};

use crate::bindings::liquidate::Liquidate;
use crate::services::indexer::PositionDelta;
use crate::services::oracle::vesu_prices::VESU_PRICES;
use crate::types::account::StarknetSingleOwnerAccount;
use crate::types::pool::PoolName;
use crate::types::{account::StarknetAccount, position::VesuPosition};

pub struct MonitoringService {
    pub vesu_client: Arc<VesuDataClient<FallbackProvider>>,
    pub rx_from_indexer: mpsc::UnboundedReceiver<(StarknetEventMetadata, PositionDelta)>,
    pub current_positions: HashMap<(PoolName, String), VesuPosition>,
    wait_for_indexer: Option<oneshot::Receiver<()>>,
    liquidate_contract: Arc<Liquidate<StarknetSingleOwnerAccount>>,
    account: StarknetAccount,
}

impl MonitoringService {
    pub fn new(
        provider: FallbackProvider,
        account: StarknetAccount,
        rx_from_indexer: mpsc::UnboundedReceiver<(StarknetEventMetadata, PositionDelta)>,
        wait_for_indexer: oneshot::Receiver<()>,
    ) -> Self {
        const LIQUIDATE_CONTRACT_ADDRESS: Felt =
            felt_hex!("0x6b895ba904fb8f02ed0d74e343161de48e611e9e771be4cc2c997501dbfb418");

        Self {
            vesu_client: Arc::new(VesuDataClient::new(StarknetNetwork::Mainnet, provider)),
            rx_from_indexer,
            current_positions: HashMap::new(),
            wait_for_indexer: Some(wait_for_indexer),
            liquidate_contract: Arc::new(Liquidate::new(
                LIQUIDATE_CONTRACT_ADDRESS,
                account.0.clone(),
            )),
            account,
        }
    }

    pub async fn run_forever(mut self) -> anyhow::Result<()> {
        tracing::info!("[🔭 Monitoring] Waiting for first vesu prices");
        VESU_PRICES.wait_for_first_prices().await;

        let wait_for_indexer = self
            .wait_for_indexer
            .take()
            .expect("wait_for_indexer should be present in the Option. The task is ran only once!");

        let mut interval = tokio::time::interval(Duration::from_secs(10));

        loop {
            tokio::select! {
                maybe_msg = self.rx_from_indexer.recv() => {
                    if let Some((metadata, event)) = maybe_msg {
                        tracing::info!("[🔭 Monitoring] Processing new event from block #{}", metadata.block_number);

                        let pool = PoolName::try_from(&metadata.from_address)?;
                        let position_key = Self::compute_position_key(metadata.from_address, &event);

                        if let Some(position) = self.current_positions.get_mut(&(pool, position_key.clone())) {
                            position.update_from_delta(event);
                        } else {
                            match VesuPosition::new(&metadata, &self.vesu_client, event).await {
                                Ok(position) => {
                                    self.current_positions.insert((pool, position.position_id()), position);
                                }
                                Err(e) => {
                                    tracing::error!("[🔭 Monitoring] Could not new create position: {e}");
                                }
                            };
                        }

                        let to_close = if let Some(position) = self.current_positions.get(&(pool, position_key.clone())) {
                            position.is_closed()
                        } else {
                            false
                        };

                        if to_close {
                            self.current_positions.remove(&(pool, position_key));
                        }


                    }
                },
                _ = interval.tick() => {
                    if wait_for_indexer.is_empty() || !self.rx_from_indexer.is_empty() {
                        continue;
                    }

                    for p in self.current_positions.values() {
                        if p.is_closed() {
                            continue;
                        }

                        if p.is_liquidable() {
                            tracing::info!(
                                "[🔭 Monitoring] 🔫 Liquidating {p}",
                            );

                            if let Err(e) = self.liquidate_position(p).await {
                                if e.to_string().contains("not-undercollateralized") {
                                    tracing::warn!("[🔭 Monitoring] Position was not under collateralized!");
                                } else {
                                    tracing::error!(
                                        error = %e,
                                        "[🔭 Monitoring] 😨 Could not liquidate position",
                                    );
                                }
                            }
                        }


                    }

                }
            }
        }
    }

    fn compute_position_key(from_address: Felt, position_event: &PositionDelta) -> String {
        let mut hasher = std::hash::DefaultHasher::new();
        vec![
            from_address,
            position_event.collateral_address,
            position_event.debt_address,
            position_event.user_address,
        ]
        .hash(&mut hasher);
        hasher.finish().to_string()
    }

    async fn liquidate_position(&self, position: &VesuPosition) -> anyhow::Result<()> {
        let started_at = std::time::Instant::now();

        let liquidation_tx = position
            .get_vesu_liquidate_tx(&self.liquidate_contract, &self.account.account_address())
            .await?;

        let tx_hash = self.account.execute_txs(&[liquidation_tx]).await?;

        tracing::info!(
            "[🔭 Monitoring] ✅ Liquidated position #{}! (tx {tx_hash:#064x}) - ⌛ {:?}",
            position.position_id(),
            started_at.elapsed()
        );
        Ok(())
    }
}
