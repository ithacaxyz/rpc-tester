//! [`RpcTester`] implementation.

use super::{MethodName, TestError};
use crate::{report::report, rpc};
use alloy_primitives::{BlockHash, BlockNumber};
use alloy_rpc_types::{
    // trace::geth::{GethDebugBuiltInTracerType, GethDebugTracerType, GethDebugTracingOptions},
    Block,
    BlockId,
    BlockNumberOrTag,
    Filter,
    Index,
    Receipt,
    Transaction,
};
use alloy_rpc_types_trace::geth::{
    GethDebugBuiltInTracerType, GethDebugTracerType, GethDebugTracingOptions,
};
use eyre::Result;
use futures::Future;
use jsonrpsee::tracing::debug;
use reth_rpc_api::{
    DebugApiClient, EthApiClient, EthFilterApiClient, RethApiClient, TraceApiClient,
};
use reth_tracing::tracing::{info, trace};
use serde::Serialize;
use std::{collections::BTreeMap, fmt::Debug, ops::RangeInclusive, pin::Pin};

// Alias type
type BlockTestResults = BTreeMap<BlockNumber, Vec<(MethodName, Result<(), TestError>)>>;

/// Type that runs queries two nodes rpc queries and ensures that the first is at least a superset
/// of the second.
#[derive(Debug)]
pub struct RpcTester<C> {
    /// First RPC node.
    rpc1: C,
    /// Second RPC node.
    rpc2: C,
    /// Whether to query tracing methods.
    use_tracing: bool,
    /// Whether to query reth namespace.
    use_reth: bool,
    /// Whether to call rpc transaction methods for every transaction. Otherwise, just the first of
    /// the block.
    use_all_txes: bool,
}

impl<C> RpcTester<C> {
    /// Returns [`RpcTesterBuilder`].
    pub const fn builder(rpc1: C, rpc2: C) -> RpcTesterBuilder<C> {
        RpcTesterBuilder::new(rpc1, rpc2)
    }
}

impl<C> RpcTester<C>
where
    C: EthApiClient<Transaction, Block, Receipt>
        + EthFilterApiClient<Transaction>
        + RethApiClient
        + TraceApiClient
        + DebugApiClient
        + Clone
        + Send
        + Sync,
{
    /// Verifies that results from `rpc1` are at least a superset of `rpc2`.
    pub async fn run(&self, block_range: RangeInclusive<BlockNumber>) -> Result<()> {
        self.test_per_block(block_range.clone()).await?;
        self.test_block_range(block_range).await?;
        Ok(())
    }

    /// Verifies RPC calls applicable to single blocks.
    async fn test_per_block(&self, block_range: RangeInclusive<u64>) -> Result<(), eyre::Error> {
        let mut results = BlockTestResults::new();

        for block_number in block_range {
            info!(block_number, "testing rpc");

            let mut tests = vec![];

            let (block, block_hash, block_tag, block_id) = self.fetch_block(block_number).await?;

            // Block based
            #[rustfmt::skip]
            tests.extend(vec![
                rpc!(self, block_by_hash, block_hash, true),
                rpc!(self, block_by_number, block_tag, true),
                rpc!(self, block_transaction_count_by_hash, block_hash),
                rpc!(self, block_transaction_count_by_number, block_tag),
                rpc!(self, block_uncles_count_by_hash, block_hash),
                rpc!(self, block_uncles_count_by_number, block_tag),
                rpc!(self, block_receipts, block_id),
                rpc!(self, header_by_number, block_tag),
                rpc!(self, header_by_hash, block_hash),
                rpc!(self, reth_get_balance_changes_in_block, block_id),
                // Response is too big & Http(TooLarge))
                // test_debug_rpc_method!(self, debug_trace_block_by_number, block_tag, None)
                rpc!(self, trace_block, block_id),
                rpc!(self, logs, Filter::new().select(block_number)),
            ]);

            // // Transaction/Receipt based RPCs
            for (index, tx) in block.transactions.into_transactions().enumerate() {
                let tracer_opts = Some(GethDebugTracingOptions::default().with_tracer(
                    GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer),
                ));
                let tx_hash = *(tx.inner.tx_hash());

                if let Some(receipt) = self.rpc2.transaction_receipt(tx_hash).await? {
                    if let Some(log) = receipt.logs.first().cloned() {
                        #[rustfmt::skip]
                        tests.push(
                            rpc!(self, logs, Filter::new().select(block_number).address(log.address))
                        );
                    }

                    if let Some(topic) =
                        receipt.logs.last().and_then(|log| log.data.topics().first()).copied()
                    {
                        #[rustfmt::skip]
                        tests.push(
                            rpc!(self, logs, Filter::new().select(block_number).event_signature(topic))
                        );
                    }
                }

                let index: Index = index.into();

                #[rustfmt::skip]
                tests.extend(vec![
                    rpc!(self, raw_transaction_by_hash, tx_hash),
                    rpc!(self, transaction_by_hash, tx_hash),
                    rpc!(self, raw_transaction_by_block_hash_and_index, block_hash,index),
                    rpc!(self, transaction_by_block_hash_and_index, block_hash, index),
                    rpc!(self, raw_transaction_by_block_number_and_index, block_tag, index ),
                    rpc!(self, transaction_by_block_number_and_index, block_tag, index ),
                    rpc!(self, transaction_receipt, tx_hash),
                    rpc!(self, transaction_count, tx.from, Some(block_id)),
                    rpc!(self, balance, tx.from, Some(block_id)),
                    rpc!(self, debug_trace_transaction, tx_hash, tracer_opts)
                ]);

                if !self.use_all_txes {
                    break;
                }
            }
            let block_results = futures::future::join_all(tests).await;
            results.insert(block_number, block_results);
        }
        report(results.into_iter().map(|(k, v)| (format!("Block Number {k}"), v)).collect())
    }

    /// Verifies RPC calls applicable to block ranges.
    async fn test_block_range(&self, block_range: RangeInclusive<u64>) -> Result<(), eyre::Error> {
        let start = *block_range.start();
        let end = *block_range.end();

        #[rustfmt::skip]
        report(vec![(
            format!("{}..={}", start, end),
            futures::future::join_all([
                rpc!(self, logs, Filter::new().from_block(start).to_block(end)
            )])
            .await,
        )])?;

        Ok(())
    }

    /// Fetches block and block identifiers from `self.truth`.
    async fn fetch_block(
        &self,
        block_number: u64,
    ) -> Result<(Block, BlockHash, BlockNumberOrTag, BlockId), eyre::Error> {
        let block: Block = self
            .rpc2
            .block_by_number(block_number.into(), true)
            .await?
            .expect("should have block from range");
        assert_eq!(block.header.number, block_number);
        let block_hash = block.header.hash;
        let block_tag = BlockNumberOrTag::Number(block_number);
        let block_id = BlockId::Number(block_tag);
        Ok((block, block_hash, block_tag, block_id))
    }

    /// Compares the response to a specific method between both rpcs. Only collects differences.
    ///
    /// If any namespace is disabled skip it.
    async fn test_rpc_call<'a, F, Fut, T, E>(
        &'a self,
        name: &str,
        method_call: F,
    ) -> (MethodName, Result<(), TestError>)
    where
        F: Fn(&'a C) -> Fut + 'a,
        Fut: std::future::Future<Output = Result<T, E>> + 'a + Send,
        T: PartialEq + Debug + Serialize,
        E: Debug,
    {
        if name.starts_with("reth") && !self.use_reth || name.contains("trace") && !self.use_tracing
        {
            return (name.to_string(), Ok(()));
        }

        trace!("## {name}");
        let t = std::time::Instant::now();
        let (rpc1_result, rpc2_result) =
            tokio::join!(method_call(&self.rpc1), method_call(&self.rpc2));
        debug!(elapsed = t.elapsed().as_millis(), ?rpc1_result, ?rpc2_result, "{name}");

        let result = match (rpc1_result, rpc2_result) {
            (Ok(rpc1), Ok(rpc2)) => {
                if rpc1 == rpc2 {
                    Ok(())
                } else {
                    Err(TestError::Diff {
                        rpc1: serde_json::to_value(&rpc1).expect("should json"),
                        rpc2: serde_json::to_value(&rpc2).expect("should json"),
                    })
                }
            }
            (Err(e), _) => Err(TestError::Rpc1Err(format!("rpc1: {e:?}"))),
            (Ok(_), Err(e)) => Err(TestError::Rpc2Err(format!("rpc2: {e:?}"))),
        };

        (name.to_string(), result)
    }
}

/// Builder for [`RpcTester`].
#[derive(Debug)]
pub struct RpcTesterBuilder<C> {
    /// First RPC node.
    rpc1: C,
    /// Second RPC node.
    rpc2: C,
    /// Whether to query tracing methods.
    use_tracing: bool,
    /// Whether to query reth namespace.
    use_reth: bool,
    /// Whether to call rpc transaction methods for every transaction. Otherwise, just the first of
    /// the block.
    use_all_txes: bool,
}

impl<C> RpcTesterBuilder<C> {
    /// Creates a new builder with default settings.
    pub const fn new(rpc1: C, rpc2: C) -> Self {
        Self { rpc1, rpc2, use_tracing: false, use_reth: false, use_all_txes: false }
    }

    /// Enables or disables tracing calls.
    pub const fn with_tracing(mut self, is_enabled: bool) -> Self {
        self.use_tracing = is_enabled;
        self
    }

    /// Enables or disables reth namespace.
    pub const fn with_reth(mut self, is_enabled: bool) -> Self {
        self.use_reth = is_enabled;
        self
    }

    /// Enables or disables querying all transactions. Will only query the first of the block if
    /// disabled.
    pub const fn with_all_txes(mut self, is_enabled: bool) -> Self {
        self.use_all_txes = is_enabled;
        self
    }

    /// Builds and returns the [`RpcTester`].
    pub fn build(self) -> RpcTester<C> {
        RpcTester {
            rpc1: self.rpc1,
            rpc2: self.rpc2,
            use_tracing: self.use_tracing,
            use_reth: self.use_reth,
            use_all_txes: self.use_all_txes,
        }
    }
}
