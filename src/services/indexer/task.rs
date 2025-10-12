use evian::utils::indexer::handler::StarknetEventMetadata;
use pragma_common::{
    services::{Service, ServiceRunner},
    starknet::FallbackProvider,
};
use tokio::sync::{mpsc, oneshot};

use crate::services::indexer::{IndexerService, PositionDelta};

pub struct IndexerTask {
    starting_block: u64,
    apibara_api_key: String,
    provider: FallbackProvider,
    tx_to_monitoring: mpsc::UnboundedSender<(StarknetEventMetadata, PositionDelta)>,
    meet_with_monitoring: Option<oneshot::Sender<()>>,
}

impl IndexerTask {
    pub fn new(
        starting_block: u64,
        apibara_api_key: String,
        provider: FallbackProvider,
        tx_to_monitoring: mpsc::UnboundedSender<(StarknetEventMetadata, PositionDelta)>,
        meet_with_monitoring: oneshot::Sender<()>,
    ) -> Self {
        Self {
            starting_block,
            apibara_api_key,
            provider,
            tx_to_monitoring,
            meet_with_monitoring: Some(meet_with_monitoring),
        }
    }
}

#[async_trait::async_trait]
impl Service for IndexerTask {
    async fn start<'a>(&mut self, mut runner: ServiceRunner<'a>) -> anyhow::Result<()> {
        let starting_block = self.starting_block;
        let apibara_api_key = self.apibara_api_key.clone();
        let provider = self.provider.clone();
        let tx_to_monitoring = self.tx_to_monitoring.clone();
        let meet_with_monitoring = self
            .meet_with_monitoring
            .take()
            .expect("IndexerTask cannot be launched twice");

        runner.spawn_loop(move |ctx| async move {
            let mut indexer_service = IndexerService::new(
                starting_block,
                apibara_api_key,
                provider,
                tx_to_monitoring,
                meet_with_monitoring,
            );
            if let Some(result) = ctx.run_until_cancelled(indexer_service.run_forever()).await {
                result?;
            }

            anyhow::Ok(())
        });

        Ok(())
    }
}
