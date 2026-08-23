use std::time::Duration;

use pragma_common::starknet::FallbackProvider;
use starknet_rust::providers::{JsonRpcClient, Url, jsonrpc::HttpTransport};

/// Deadline applied to every outbound HTTP call.
///
/// Without it a silently stalled connection blocks the calling task forever:
/// `reqwest::Client::new()` has no timeout at all, and `FallbackProvider` only
/// fails over on *errors*, never on a hang.
pub const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Builds an HTTP client that always gives up instead of hanging.
pub fn http_client(request_timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(HTTP_CONNECT_TIMEOUT.min(request_timeout))
        .timeout(request_timeout)
        .build()
}

/// Builds the Starknet provider used by every service.
///
/// `FallbackProvider::new` would build its transports with an unbounded
/// `reqwest::Client`, so the clients are assembled here instead.
pub fn build_provider(
    urls: Vec<Url>,
    request_timeout: Duration,
) -> anyhow::Result<FallbackProvider> {
    anyhow::ensure!(!urls.is_empty(), "at least one RPC url is required");

    let client = http_client(request_timeout)?;
    Ok(FallbackProvider::from_clients(
        urls.into_iter()
            .map(|url| JsonRpcClient::new(HttpTransport::new_with_client(url, client.clone())))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use starknet_rust::{
        core::types::{BlockId, BlockTag, Felt, FunctionCall},
        providers::Provider,
    };

    use super::*;

    /// A stalled endpoint must not wedge the caller: `FallbackProvider` only
    /// fails over on errors, so the deadline has to come from the HTTP client.
    #[tokio::test]
    async fn rpc_call_gives_up_on_a_stalled_endpoint() {
        const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        // Accepts, then never answers: the connection is alive but mute.
        tokio::spawn(async move {
            let mut accepted = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                accepted.push(socket);
            }
        });

        let provider = build_provider(
            vec![format!("http://{address}").parse().unwrap()],
            REQUEST_TIMEOUT,
        )
        .unwrap();

        let call = FunctionCall {
            contract_address: Felt::ONE,
            entry_point_selector: Felt::ONE,
            calldata: vec![],
        };

        let outcome = tokio::time::timeout(
            REQUEST_TIMEOUT * 10,
            provider.call(call, BlockId::Tag(BlockTag::Latest)),
        )
        .await;

        assert!(
            matches!(outcome, Ok(Err(_))),
            "the call must return an error instead of hanging"
        );
    }
}
