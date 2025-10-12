use pragma_common::{
    services::{Service, ServiceRunner},
    starknet::FallbackProvider,
};

use crate::services::oracle::OracleService;

pub struct OracleTask {
    starknet_provider: FallbackProvider,
}

impl OracleTask {
    pub const fn new(starknet_provider: FallbackProvider) -> Self {
        Self { starknet_provider }
    }
}

#[async_trait::async_trait]
impl Service for OracleTask {
    async fn start<'a>(&mut self, mut runner: ServiceRunner<'a>) -> anyhow::Result<()> {
        let starknet_provider = self.starknet_provider.clone();

        runner.spawn_loop(move |ctx| async move {
            let oracle_service = OracleService::new(starknet_provider);
            if let Some(result) = ctx.run_until_cancelled(oracle_service.run_forever()).await {
                result?;
            }

            anyhow::Ok(())
        });

        Ok(())
    }
}
