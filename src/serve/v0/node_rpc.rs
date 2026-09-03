use std::sync::Arc;

use bitcoincore_rpc::{Auth, Client, RpcApi, jsonrpc};
use serde_json::Value;
use tokio::task::spawn_blocking;

use crate::error::Error;
use crate::serve::error::ServeError;
use crate::sync::NodeConfig;

/// Bitcoin Core RPC error code for a transaction that is neither in the
/// mempool nor (with -txindex) in a block.
const RPC_INVALID_ADDRESS_OR_KEY: i32 = -5;

/// Bitcoin Core RPC error code for an out-of-range or malformed parameter.
const RPC_INVALID_PARAMETER: i32 = -8;

#[derive(Debug)]
pub struct RpcCallError {
    pub code: Option<i32>,
    pub message: String,
}

impl RpcCallError {
    pub fn is_not_found(&self) -> bool {
        self.code == Some(RPC_INVALID_ADDRESS_OR_KEY)
    }

    pub fn is_invalid_parameter(&self) -> bool {
        self.code == Some(RPC_INVALID_PARAMETER)
    }

    /// Codes sendrawtransaction returns for a transaction the node evaluated
    /// and refused. Anything else with a code (warmup, IBD, internal node
    /// faults) is not the transaction's fault and must not read as a
    /// permanent rejection.
    pub fn is_transaction_rejection(&self) -> bool {
        const REJECTION_CODES: [i32; 5] = [-8, -22, -25, -26, -27];
        self.code.is_some_and(|c| REJECTION_CODES.contains(&c))
    }

    pub fn into_serve_error(self) -> ServeError {
        if self.is_not_found() {
            ServeError::NotFound
        } else {
            ServeError::internal(format!("node rpc: {}", self.message))
        }
    }
}

impl From<bitcoincore_rpc::Error> for RpcCallError {
    fn from(error: bitcoincore_rpc::Error) -> Self {
        let code = match &error {
            bitcoincore_rpc::Error::JsonRpc(jsonrpc::Error::Rpc(rpc)) => Some(rpc.code),
            _ => None,
        };

        Self {
            code,
            message: error.to_string(),
        }
    }
}

/// Blocking Bitcoin Core JSON-RPC client shared by the v0 compatibility
/// routes; every call runs on the blocking thread pool.
pub struct NodeRpc {
    client: Arc<Client>,
}

impl NodeRpc {
    pub fn new(config: &NodeConfig) -> Result<Self, Error> {
        let client = Client::new(
            &config.rpc_address,
            Auth::UserPass(config.rpc_user.clone(), config.rpc_pass.clone()),
        )
        .map_err(|e| Error::Config(format!("node rpc client: {e}")))?;

        Ok(Self {
            client: Arc::new(client),
        })
    }

    pub async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, RpcCallError> {
        let client = self.client.clone();
        let method = method.to_string();

        spawn_blocking(move || {
            client
                .call::<Value>(&method, &params)
                .map_err(RpcCallError::from)
        })
        .await
        .map_err(|e| RpcCallError {
            code: None,
            message: format!("rpc task join: {e}"),
        })?
    }
}
