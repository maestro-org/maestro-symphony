use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// -- core types

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServeResponse<T> {
    pub data: T,
    pub indexer_info: IndexerInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IndexerInfo {
    /// Most recent *mined* block in the indexed mainchain, any estimated blocks will be descendants of this block
    pub chain_tip: ChainTip,
    /// Timestamp of the indexed mempool snapshot, if any estimated blocks from the mempool have been indexed
    pub mempool_timestamp: Option<String>,
    /// Information about any estimated blocks from the mempool that were indexed in addition to the mainchain
    pub estimated_blocks: Vec<EstimatedBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChainTip {
    /// The hash of the block
    pub block_hash: String,

    /// The height of the block in the blockchain
    pub block_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EstimatedBlock {
    pub block_height: u64,
}

#[derive(Deserialize, ToSchema)]
pub struct MempoolParam {
    #[serde(default)]
    pub mempool: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct RuneBalancesParam {
    #[serde(default)]
    pub mempool: bool,
    #[serde(default)]
    pub include_info: bool,
}

// -- endpoint types

#[derive(Serialize, ToSchema)]
pub struct RuneAndAmount {
    pub id: String,
    pub amount: String,
}

#[derive(Serialize, ToSchema)]
pub struct RuneBalanceWithInfo {
    pub id: String,
    pub amount: String,
    pub info: Option<RuneInfo>,
}

#[derive(Serialize, ToSchema)]
pub struct RuneUtxo {
    pub tx_hash: String,
    pub output_index: u32,
    pub height: u64,
    pub satoshis: String,
    pub runes: Vec<RuneAndAmount>,
}

#[derive(Serialize, ToSchema)]
pub struct AddressUtxo {
    pub tx_hash: String,
    pub output_index: u32,
    pub height: u64,
    pub satoshis: String,
}

#[derive(Serialize, ToSchema)]
pub struct RuneEdict {
    pub rune_id: String,
    pub amount: String,
    pub output: Option<u32>,
    pub block_height: u64,
}

#[derive(Serialize, ToSchema)]
pub struct RuneInfo {
    pub id: String,
    pub name: String,
    pub spaced_name: String,
    pub symbol: Option<char>,
    pub divisibility: u8,
    pub etching_tx: String,
    pub etching_height: u64,
    pub terms: Option<RuneTerms>,
    pub premine: String,
}

#[derive(Serialize, ToSchema)]
pub struct RuneTerms {
    pub amount: Option<String>,
    pub cap: Option<String>,
    pub start_height: Option<u64>,
    pub end_height: Option<u64>,
}
