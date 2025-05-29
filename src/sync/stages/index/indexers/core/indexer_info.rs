use maestro_symphony_macros::{Decode, Encode};

use crate::{
    define_core_table,
    error::Error,
    storage::{kv_store::Task, table::CoreTable},
};

use super::CoreIndexer;

define_core_table! {
    name: IndexerInfoKV,
    key_type: (),
    value_type: IndexerInfo,
    indexer: CoreIndexer::IndexerInfo
}

#[derive(Encode, Decode)]
pub struct IndexerInfo {
    last_point_height: u64,
    last_point_hash: [u8; 32],
    mempool_info: u64, // TODO
}

impl IndexerInfo {
    // TODO
    pub fn new() -> Self {
        IndexerInfo {
            last_point_height: 0,
            last_point_hash: [0; 32],
            mempool_info: 0,
        }
    }
}

impl IndexerInfoKV {
    pub fn set_info(task: &mut Task, info: IndexerInfo) -> Result<(), Error> {
        task.set::<Self>((), info)?;

        Ok(())
    }
}
