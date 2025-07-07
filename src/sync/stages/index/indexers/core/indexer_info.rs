use maestro_symphony_macros::{Decode, Encode};

use crate::{
    define_core_table,
    error::Error,
    storage::{kv_store::IndexingTask, table::CoreTable},
};

use super::CoreIndexer;

define_core_table! {
    name: IndexerInfoKV,
    key_type: (),
    value_type: IndexerInfo,
    indexer: CoreIndexer::IndexerInfo
}

#[derive(Encode, Decode, Debug)]
pub struct IndexerInfo {
    pub last_point_height: u64,
    pub last_point_hash: [u8; 32],
    pub mempool: bool,
}

impl IndexerInfo {
    pub fn new(height: u64, hash: [u8; 32], mempool: bool) -> Self {
        IndexerInfo {
            last_point_height: height,
            last_point_hash: hash,
            mempool,
        }
    }
}

impl IndexerInfoKV {
    pub fn set_info(task: &mut IndexingTask, info: IndexerInfo) -> Result<(), Error> {
        task.set::<Self>((), info)?;

        Ok(())
    }
}
