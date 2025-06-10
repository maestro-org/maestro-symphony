use maestro_symphony_macros::{Decode, Encode};

use crate::{
    define_core_table,
    storage::{kv_store::PreviousValue, table::CoreTable},
    sync::stages::BlockHeight,
};

use super::CoreIndexer;

define_core_table! {
    name: RollbackBufferKV,
    key_type: RollbackKey,
    value_type: PreviousValue,
    indexer: CoreIndexer::RollbackBuffer
}

#[derive(Encode, Decode, PartialEq, Hash, Eq, Clone, Debug)]
pub struct RollbackKey {
    pub height: BlockHeight,
    pub hash: [u8; 32],
    pub key: Vec<u8>,
}
