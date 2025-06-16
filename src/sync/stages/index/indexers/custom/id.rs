use crate::{
    error::Error,
    storage::kv_store::IndexingTask,
    sync::stages::{TransactionWithId, index::worker::context::IndexingContext},
};

use super::{BlockIndexer, TransactionIndexer};

pub trait ProcessTransaction {
    fn process_tx(
        &self,
        task: &mut IndexingTask,
        tx: &TransactionWithId,
        tx_block_index: usize,
        ctx: &mut IndexingContext,
    ) -> Result<(), Error>;
}

pub trait IndexerIdentifier {
    fn unique_id(&self) -> u16;
}

impl IndexerIdentifier for TransactionIndexer {
    fn unique_id(&self) -> u16 {
        *self as u16
    }
}

impl IndexerIdentifier for BlockIndexer {
    fn unique_id(&self) -> u16 {
        0xFF00 | *self as u16
    }
}
