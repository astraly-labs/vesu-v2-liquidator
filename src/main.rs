pub mod bindings;
pub mod cli;
pub mod config;
pub mod services;
pub mod types;
pub mod utils;

use clap::Parser;
use pragma_common::services::{Service, ServiceGroup};
use pragma_common::starknet::FallbackProvider;
use pragma_common::telemetry::init_telemetry;
use tokio::sync::{mpsc, oneshot};

use crate::cli::RunCmd;
use crate::services::indexer::task::IndexerTask;
use crate::services::monitoring::task::MonitoringTask;
use crate::services::oracle::task::OracleTask;
use crate::types::account::StarknetAccount;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_telemetry("vesu-v2-liquidator", None).expect("Could not init telemetry");

    let _ = dotenvy::dotenv();

    let mut run_cmd = RunCmd::parse();
    run_cmd.validate()?;

    print_app_title();

    let provider = FallbackProvider::new(vec![
        run_cmd.rpc_url.clone(),
        "https://api.cartridge.gg/x/starknet/mainnet"
            .parse()
            .expect("Coudlnt parse Cartridge RPC URL?"),
        "https://rpc.pathfinder.equilibrium.co/mainnet/rpc/v0_9"
            .parse()
            .expect("Coudlnt parse Equilibrium RPC URL?"),
    ])
    .expect("Could not init the Starknet provider");

    let account = StarknetAccount::from_cli(provider.clone(), run_cmd.clone())?;

    let oracle_service = OracleTask::new(provider.clone());

    let (meet_with_monitoring, wait_for_indexer) = oneshot::channel::<()>();
    let (tx_to_monitoring, rx_from_indexer) = mpsc::unbounded_channel();

    let indexer_service = IndexerTask::new(
        run_cmd.starting_block,
        run_cmd.apibara_api_key,
        provider.clone(),
        tx_to_monitoring,
        meet_with_monitoring,
    );

    let monitoring_service =
        MonitoringTask::new(account, provider.clone(), rx_from_indexer, wait_for_indexer);

    ServiceGroup::default()
        .with(oracle_service)
        .with(indexer_service)
        .with(monitoring_service)
        .start_and_drive_to_end()
        .await?;

    Ok(())
}

/// Prints information about the bot parameters.
fn print_app_title() {
    println!("\n
██╗   ██╗███████╗███████╗██╗   ██╗    ██╗     ██╗ ██████╗ ██╗   ██╗██╗██████╗  █████╗ ████████╗ ██████╗ ██████╗
██║   ██║██╔════╝██╔════╝██║   ██║    ██║     ██║██╔═══██╗██║   ██║██║██╔══██╗██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗
██║   ██║█████╗  ███████╗██║   ██║    ██║     ██║██║   ██║██║   ██║██║██║  ██║███████║   ██║   ██║   ██║██████╔╝
╚██╗ ██╔╝██╔══╝  ╚════██║██║   ██║    ██║     ██║██║▄▄ ██║██║   ██║██║██║  ██║██╔══██║   ██║   ██║   ██║██╔══██╗
 ╚████╔╝ ███████╗███████║╚██████╔╝    ███████╗██║╚██████╔╝╚██████╔╝██║██████╔╝██║  ██║   ██║   ╚██████╔╝██║  ██║
  ╚═══╝  ╚══════╝╚══════╝ ╚═════╝     ╚══════╝╚═╝ ╚══▀▀═╝  ╚═════╝ ╚═╝╚═════╝ ╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝  ╚═╝
  
  -----------------------------------------------------
  ");
}
