pub mod account;

use anyhow::{Result, anyhow};
use url::Url;

use crate::cli::account::AccountParams;

fn parse_url(s: &str) -> Result<Url> {
    s.parse()
        .map_err(|_| anyhow!("Could not convert {s} to Url"))
}

#[derive(Clone, Debug, clap::Parser)]
pub struct RunCmd {
    #[allow(missing_docs)]
    #[clap(flatten)]
    pub account_params: AccountParams,

    /// The rpc endpoint url.
    #[clap(long, value_parser = parse_url, value_name = "RPC URL", env = "RPC_URL")]
    pub rpc_url: Url,

    /// The block you want to start syncing from.
    #[clap(
        long,
        short,
        value_name = "BLOCK NUMBER",
        env = "STARTING_BLOCK",
        default_value = "2383614"
    )]
    pub starting_block: u64,

    /// Apibara API Key for indexing.
    #[clap(long, value_name = "APIBARA API KEY", env = "APIBARA_API_KEY")]
    pub apibara_api_key: String,
}

impl RunCmd {
    pub fn validate(&mut self) -> Result<()> {
        self.account_params.validate()?;
        Ok(())
    }
}
