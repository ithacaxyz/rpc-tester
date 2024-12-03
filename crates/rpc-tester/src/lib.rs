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

/// RPC macro that boxes all method future results.
#[macro_export]
macro_rules! rpc {
    ($self:expr, $method:ident $(, $args:expr )* ) => {
        Box::pin($self.test_rpc_call(
            stringify!($method),
            move |client: &C|  {
                client.$method( $( $args.clone(), )*)
            }
        )) as Pin<Box<dyn Future<Output = ($crate::MethodName, Result<(), $crate::TestError>)> + Send>>
    };
}
