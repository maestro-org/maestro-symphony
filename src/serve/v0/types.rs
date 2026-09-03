use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::serve::types::{IndexerInfo, SortOrder};

/// Indexer chain tip in the `last_updated` shape of the hosted rpc/*
/// endpoints (height before hash).
#[derive(Debug, Clone, Serialize)]
pub struct V0BlockPointer {
    pub block_height: u64,
    pub block_hash: String,
}

impl From<&IndexerInfo> for V0BlockPointer {
    fn from(info: &IndexerInfo) -> Self {
        Self {
            block_height: info.chain_tip.block_height,
            block_hash: info.chain_tip.block_hash.clone(),
        }
    }
}

/// Indexer chain tip in the `last_updated` shape of the hosted address
/// endpoints, which serialize hash before height.
#[derive(Debug, Clone, Serialize)]
pub struct V0AddressTip {
    pub block_hash: String,
    pub block_height: u64,
}

impl From<&IndexerInfo> for V0AddressTip {
    fn from(info: &IndexerInfo) -> Self {
        Self {
            block_hash: info.chain_tip.block_hash.clone(),
            block_height: info.chain_tip.block_height,
        }
    }
}

#[derive(Serialize)]
pub struct V0DataResponse<T> {
    pub data: T,
    pub last_updated: V0BlockPointer,
}

#[derive(Serialize)]
pub struct V0PaginatedResponse<T> {
    pub data: T,
    pub last_updated: V0AddressTip,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct V0MempoolPaginatedResponse<T> {
    pub data: T,
    pub indexer_info: IndexerInfo,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct V0AddressTx {
    pub tx_hash: String,
    pub height: u64,
    pub input: bool,
    pub output: bool,
}

#[derive(Serialize)]
pub struct V0ConfirmedUtxo {
    pub txid: String,
    pub vout: u32,
    pub address: String,
    pub script_pubkey: String,
    pub satoshis: String,
    pub confirmations: u64,
    pub height: u64,
    pub runes: Vec<Value>,
    pub inscriptions: Vec<Value>,
}

#[derive(Serialize)]
pub struct V0MempoolUtxo {
    pub txid: String,
    pub vout: u32,
    pub address: String,
    pub script_pubkey: String,
    pub satoshis: String,
    pub height: u64,
    pub mempool: bool,
    pub runes: Vec<Value>,
    pub inscriptions: Vec<Value>,
}

/// `feerate` is a Value so a missing node estimate serializes as the integer
/// zero the hosted API emits, while real estimates stay floats.
#[derive(Serialize)]
pub struct V0FeeEstimate {
    pub feerate: Value,
    pub blocks: u64,
}

#[derive(Deserialize)]
pub struct V0TxsParams {
    pub count: Option<usize>,
    pub order: Option<SortOrder>,
    pub cursor: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub confirmations: Option<u64>,
}

#[derive(Deserialize)]
pub struct V0UtxosParams {
    pub count: Option<usize>,
    pub order: Option<SortOrder>,
    pub cursor: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub filter_dust: Option<bool>,
    pub filter_dust_threshold: Option<u64>,
    #[allow(dead_code)]
    pub exclude_metaprotocols: Option<bool>,
    #[allow(dead_code)]
    pub mempool_blocks_limit: Option<u64>,
}

#[derive(Deserialize)]
pub struct V0FeeModeParam {
    pub mode: Option<String>,
}
