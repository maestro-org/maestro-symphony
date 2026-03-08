use crate::error::Error;
use crate::storage::kv_store::IndexingTask;
use crate::sync::stages::index::indexers::custom::id::ProcessTransaction;
use crate::sync::stages::index::worker::context::IndexingContext;
use crate::sync::stages::TransactionWithId;

pub struct CharmsIndexer {
}

impl CharmsIndexer {
    pub fn new() -> Self {
        CharmsIndexer {}
    }
}

impl ProcessTransaction for CharmsIndexer {
    fn process_tx(&self, task: &mut IndexingTask, tx: &TransactionWithId, tx_block_index: usize, ctx: &mut IndexingContext) -> Result<(), Error> {
        todo!()
    }
}