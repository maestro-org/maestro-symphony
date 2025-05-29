use maestro_symphony_macros::{Decode, Encode};

use crate::{define_core_table, storage::table::CoreTable};

use super::CoreIndexer;

define_core_table! {
    name: PointByTimestampKV,
    key_type: u64,
    value_type: PointInfo,
    indexer: CoreIndexer::PointByTimestamp
}

#[derive(Encode, Decode)]
pub enum PointInfo {
    Block { height: u64, hash: [u8; 32] },
    Mempool(()), // TODO
}
