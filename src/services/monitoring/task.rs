use evian::utils::indexer::handler::StarknetEventMetadata;
use pragma_common::{
    services::{Service, ServiceRunner},
    starknet::FallbackProvider,
};
use tokio::sync::{mpsc, oneshot};

use crate::{
    services::{indexer::PositionDelta, monitoring::MonitoringService},
    types::account::StarknetAccount,
};

pub struct MonitoringTask {
    account: StarknetAccount,
    provider: FallbackProvider,
    rx_from_indexer: Option<mpsc::UnboundedReceiver<(StarknetEventMetadata, PositionDelta)>>,
    wait_for_indexer: Option<oneshot::Receiver<()>>,
}

impl MonitoringTask {
    pub fn new(
        account: StarknetAccount,
        provider: FallbackProvider,
        rx_from_indexer: mpsc::UnboundedReceiver<(StarknetEventMetadata, PositionDelta)>,
        wait_for_indexer: oneshot::Receiver<()>,
    ) -> Self {
        Self {
            account,
            provider,
            rx_from_indexer: Some(rx_from_indexer),
            wait_for_indexer: Some(wait_for_indexer),
        }
    }
}

#[async_trait::async_trait]
impl Service for MonitoringTask {
    async fn start<'a>(&mut self, mut runner: ServiceRunner<'a>) -> anyhow::Result<()> {
        let account = self.account.clone();
        let provider = self.provider.clone();
        let rx_from_indexer = self
            .rx_from_indexer
            .take()
            .expect("MonitoringTask cannot be launched twice");
        let wait_for_indexer = self
            .wait_for_indexer
            .take()
            .expect("MonitoringTask cannot be launched twice");

        runner.spawn_loop(move |ctx| async move {
            let monitoring_service =
                MonitoringService::new(provider, account, rx_from_indexer, wait_for_indexer);
            if let Some(result) = ctx
                .run_until_cancelled(monitoring_service.run_forever())
                .await
            {
                result?;
            }

            anyhow::Ok(())
        });

        Ok(())
    }
}
