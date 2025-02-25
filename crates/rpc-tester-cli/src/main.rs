//! CLI definition and entrypoint to executable

use alloy_provider::{network::AnyNetwork, Provider, ProviderBuilder};
use alloy_rpc_types::SyncStatus;
use clap::Parser;
use reth_tracing::{tracing::info, RethTracer, Tracer};
use rpc_tester::RpcTester;
use std::{ops::RangeInclusive, thread::sleep, time::Duration};
use url::Url;

/// The rpc-tester-cli interface.
#[derive(Debug, Parser)]
#[command(about = "Verifies that results from `rpc1` are at the very least a superset of `rpc2`")]
pub struct CliArgs {
    /// RPC URL 1
    #[arg(long, value_name = "RPC_URL1", value_parser = Url::parse)]
    pub rpc1: Url,

    /// RPC URL 2
    #[arg(long, value_name = "RPC_URL2")]
    pub rpc2: Url,

    /// Number of blocks to test from the tip.
    #[arg(long, value_name = "NUM_BLOCKS", default_value = "32")]
    pub num_blocks: u64,

    /// Whether to query reth namespace
    #[arg(long, value_name = "RETH", default_value = "false")]
    pub use_reth: bool,

    /// Whether to query tracing methods
    #[arg(long, value_name = "TRACING", default_value = "false")]
    pub use_tracing: bool,

    /// Whether to query every transacion from a block or just the first.
    #[arg(long, value_name = "ALL_TXES", default_value = "false")]
    pub use_all_txes: bool,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    RethTracer::new().init()?;

    let args = CliArgs::parse();

    let rpc1 = ProviderBuilder::new()
        .disable_recommended_fillers()
        .network::<AnyNetwork>()
        .on_http(args.rpc1);
    let rpc2 = ProviderBuilder::new()
        .disable_recommended_fillers()
        .network::<AnyNetwork>()
        .on_http(args.rpc2);

    let block_range = wait_for_readiness(&rpc1, &rpc2, args.num_blocks).await?;

    RpcTester::builder(rpc1, rpc2)
        .with_tracing(args.use_tracing)
        .with_reth(args.use_reth)
        .with_all_txes(args.use_all_txes)
        .build()
        .run(block_range)
        .await
}

/// Waits until rpc1 is synced to the tip and returns a valid block range to test against rpc2.
pub async fn wait_for_readiness<P: Provider<AnyNetwork>>(
    rpc1: &P,
    rpc2: &P,
    block_size_range: u64,
) -> eyre::Result<RangeInclusive<u64>> {
    let sleep = || sleep(Duration::from_secs(5));

    // Waits until it's done syncing
    while let SyncStatus::Info(sync_info) = rpc1.syncing().await? {
        info!(?sync_info, "rpc1 still syncing");
        sleep();
    }

    // Waits until rpc1 has _mostly_ catch up to rpc2 or beyond
    loop {
        let tip1 = rpc1.get_block_number().await?;
        let tip2 = rpc2.get_block_number().await?;

        if tip1 >= tip2 || tip2 - tip1 <= 5 {
            let range = tip2 - (block_size_range - 1)..=tip2;
            info!(?range, "testing block range");
            return Ok(range);
        }

        sleep();
    }
}
