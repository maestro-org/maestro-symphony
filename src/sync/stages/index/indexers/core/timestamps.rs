use maestro_symphony_macros::{Decode, Encode};

use crate::{define_core_table, storage::table::CoreTable};

use super::CoreIndexer;

define_core_table! {
    name: TimestampsKV,
    key_type: PointKind,
    value_type: TimestampEntry,
    indexer: CoreIndexer::Timestamps
}

/// There is only two keys in the TimestampKV:
/// - 'confirmed' to get timestamp for data as of greatest confirmed block
/// - 'mempool' to get timestamp for data as of most recent mempool refresh, if available
#[derive(Encode, Decode, Debug)]
pub enum PointKind {
    Confirmed,
    Mempool,
}

#[derive(Encode, Decode, Debug)]
pub struct TimestampEntry {
    pub tip_height: u64,
    pub tip_hash: [u8; 32],
    pub timestamp: u64,
}
