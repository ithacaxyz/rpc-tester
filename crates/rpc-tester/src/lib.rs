//! rpc-tester library
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

mod tester;
pub use tester::RpcTester;
mod report;

/// Equality rpc test error
enum TestError {
    Diff { rpc1: serde_json::Value, rpc2: serde_json::Value },
    Rpc1Err(String),
    Rpc2Err(String),
}

/// Alias type
type ReportResults = Vec<(String, Vec<(MethodName, Result<(), TestError>)>)>;

/// Alias type
type MethodName = String;

/// Provider macro that boxes all method future results.
#[macro_export]
macro_rules! rpc {
    ($self:expr, $method:ident $(, $args:expr )* ) => {
        Box::pin($self.test_rpc_call(
            stringify!($method),
            move |provider: &P| {
                provider.$method( $( $args.clone(), )*)
            }
        )) as Pin<Box<dyn Future<Output = (MethodName, Result<(), TestError>)> + Send>>
    };
}

/// Provider macro to call RpcWithBlock methods and boxes all future results.
#[macro_export]
macro_rules! rpc_with_block {
    ($self:expr, $method:ident $(, $args:expr )*; $blockid:expr) => {
        Box::pin($self.test_rpc_call(
            stringify!($method),
            move |provider: &P| {
                provider.$method( $( $args.clone(), )*).block_id($blockid).into_future()
            }
        )) as Pin<Box<dyn Future<Output = (MethodName, Result<(), TestError>)> + Send>>
    };
}

/// Macro to call the `get_logs` rpc method and box the future result.
#[macro_export]
macro_rules! get_logs {
    ($self:expr, $arg:expr) => {
        Box::pin(async move {
            let filter = $arg.clone();
            $self
                .test_rpc_call(stringify!(get_logs), move |provider: &P| {
                    let filter = filter.clone();
                    async move { provider.get_logs(&filter).await }
                })
                .await
        }) as Pin<Box<dyn Future<Output = (MethodName, Result<(), TestError>)> + Send>>
    };
}
