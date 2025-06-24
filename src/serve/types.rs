use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeResponse<T> {
    pub data: T,
    pub indexer_info: IndexerInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerInfo {
    /// Most recent *mined* block in the indexed mainchain, any estimated blocks will be descendants of this block
    pub chain_tip: ChainTip,
    /// Timestamp of the indexed mempool snapshot, if any estimated blocks from the mempool have been indexed
    pub mempool_timestamp: Option<String>,
    /// Information about any estimated blocks from the mempool that were indexed in addition to the mainchain
    pub estimated_blocks: Vec<EstimatedBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainTip {
    /// The hash of the block
    pub block_hash: String,

    /// The height of the block in the blockchain
    pub block_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EstimatedBlock {
    pub block_height: u64,
}
