pub mod ekubo;
pub mod task;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use evian::{
    utils::starknet_indexer::handler::StarknetEventMetadata, vesu_v2::data::VesuDataClient,
};
use pragma_common::starknet::{FallbackProvider, StarknetNetwork};
use starknet_rust::core::types::Felt;
use starknet_rust::macros::felt_hex;
use tokio::sync::{mpsc, oneshot};

use crate::bindings::liquidate::Liquidate;
use crate::services::indexer::PositionDelta;
use crate::services::oracle::vesu_prices::VESU_PRICES;
use crate::types::account::StarknetSingleOwnerAccount;
use crate::types::pool::PoolName;
use crate::types::position::PositionKey;
use crate::types::{account::StarknetAccount, position::VesuPosition};

pub struct MonitoringService {
    pub vesu_client: Arc<VesuDataClient<FallbackProvider>>,
    pub rx_from_indexer: mpsc::UnboundedReceiver<(StarknetEventMetadata, PositionDelta)>,
    pub current_positions: HashMap<PositionKey, VesuPosition>,
    /// Cooldown per position after a failed liquidation, so a position that keeps
    /// reverting is not retried on every tick forever.
    retry_after: HashMap<PositionKey, (Instant, Duration)>,
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
            retry_after: HashMap::new(),
            wait_for_indexer: Some(wait_for_indexer),
            liquidate_contract: Arc::new(Liquidate::new(
                LIQUIDATE_CONTRACT_ADDRESS,
                account.0.clone(),
            )),
            account,
        }
    }

    pub async fn run_forever(mut self) -> anyhow::Result<()> {
        const FIRST_PRICES_TIMEOUT: Duration = Duration::from_secs(60);

        tracing::info!("[🔭 Monitoring] Waiting for first vesu prices");
        VESU_PRICES
            .wait_for_first_prices(FIRST_PRICES_TIMEOUT)
            .await;

        let wait_for_indexer = self
            .wait_for_indexer
            .take()
            .expect("wait_for_indexer should be present in the Option. The task is ran only once!");

        // `Burst` (the default) would replay every tick missed while a liquidation
        // pass was running, back to back and with no pacing.
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                maybe_msg = self.rx_from_indexer.recv() => {
                    // A closed channel means the indexer is gone: without this the arm
                    // would resolve instantly forever and spin the loop on a core.
                    let Some((metadata, event)) = maybe_msg else {
                        anyhow::bail!("[🔭 Monitoring] The indexer channel closed");
                    };

                    tracing::info!("[🔭 Monitoring] Processing new event from block #{}", metadata.block_number);

                    let pool = match PoolName::try_from(&metadata.from_address) {
                        Ok(pool) => pool,
                        Err(e) => {
                            tracing::error!("[🔭 Monitoring] Ignoring event from an unknown pool: {e}");
                            continue;
                        }
                    };
                    let position_key = PositionKey::from_delta(pool, &event);

                    if let Some(position) = self.current_positions.get_mut(&position_key) {
                        position.update_from_delta(event);
                    } else {
                        match VesuPosition::new(&metadata, &self.vesu_client, event).await {
                            Ok(position) => {
                                self.current_positions.insert(position.key(), position);
                            }
                            Err(e) => {
                                tracing::error!("[🔭 Monitoring] Could not create new position: {e}");
                            }
                        }
                    }

                    if self
                        .current_positions
                        .get(&position_key)
                        .is_some_and(VesuPosition::is_closed)
                    {
                        self.current_positions.remove(&position_key);
                        self.retry_after.remove(&position_key);
                    }
                },
                _ = interval.tick() => {
                    if wait_for_indexer.is_empty() || !self.rx_from_indexer.is_empty() {
                        continue;
                    }

                    // Collected first: liquidating borrows `self`, and the cooldown map
                    // is written right after each attempt.
                    let now = Instant::now();
                    let due: Vec<PositionKey> = self
                        .current_positions
                        .iter()
                        .filter(|(key, position)| {
                            !position.is_closed()
                                && self.retry_after.get(*key).is_none_or(|(until, _)| now >= *until)
                                && position.is_liquidable()
                        })
                        .map(|(key, _)| *key)
                        .collect();

                    for key in due {
                        // Re-checked here, not just in the filter above: an earlier
                        // liquidation in this batch can take minutes, and prices refresh
                        // every 10s. Never submit on a verdict from a previous batch.
                        let Some(position) = self
                            .current_positions
                            .get(&key)
                            .filter(|position| !position.is_closed() && position.is_liquidable())
                            .cloned()
                        else {
                            continue;
                        };

                        tracing::info!("[🔭 Monitoring] 🔫 Liquidating {position}");

                        match self.liquidate_position(&position).await {
                            Ok(()) => {
                                self.retry_after.remove(&key);
                            }
                            Err(e) if e.to_string().contains("not-undercollateralized") => {
                                // A lost race, not a failure: someone liquidated first, or
                                // the price recovered. Backing off here would blind us to
                                // exactly the positions sitting on their threshold.
                                tracing::warn!("[🔭 Monitoring] Position was not under collateralized!");
                                self.retry_after.remove(&key);
                            }
                            Err(e) => {
                                tracing::error!(
                                    error = %e,
                                    "[🔭 Monitoring] 😨 Could not liquidate position",
                                );
                                self.back_off(key);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Doubles this position's cooldown, so a position that keeps failing stops
    /// costing an Ekubo quote and a fee estimate on every tick.
    fn back_off(&mut self, key: PositionKey) {
        const MIN_BACKOFF: Duration = Duration::from_secs(30);
        const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);

        let delay = match self.retry_after.get(&key) {
            Some((_, previous)) => (*previous * 2).min(MAX_BACKOFF),
            None => MIN_BACKOFF,
        };
        tracing::warn!("[🔭 Monitoring] Backing off this position for {delay:?}");
        self.retry_after
            .insert(key, (Instant::now() + delay, delay));
    }

    async fn liquidate_position(&self, position: &VesuPosition) -> anyhow::Result<()> {
        let started_at = Instant::now();

        let liquidation_tx = position
            .get_vesu_liquidate_tx(&self.liquidate_contract, &self.account.account_address())
            .await?;

        let tx_hash = self.account.execute_txs(&[liquidation_tx]).await?;

        tracing::info!(
            "[🔭 Monitoring] ✅ Liquidated {position}! (tx {tx_hash:#064x}) - ⌛ {:?}",
            started_at.elapsed()
        );
        Ok(())
    }
}
