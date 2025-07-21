impl Default for TxCountByAddressIndexer {
    fn default() -> Self {
        Self::new()
    }
}
use std::collections::HashSet;

use super::id::ProcessTransaction;
use crate::define_indexer_table;
use crate::error::Error;
use crate::storage::kv_store::IndexingTask;
use crate::storage::table::IndexerTable;
use crate::sync::stages::TransactionWithId;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::types::ScriptPubKey;
use crate::sync::stages::index::worker::context::IndexingContext;

// --- storage

define_indexer_table! {
    name: TxCountByAddressKV,
    key_type: ScriptPubKey,
    value_type: u64,
    indexer: TransactionIndexer::TxCountByAddress,
    table: 0
}

// --- indexer

pub struct TxCountByAddressIndexer;

impl TxCountByAddressIndexer {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessTransaction for TxCountByAddressIndexer {
    fn process_tx(
        &self,
        task: &mut IndexingTask,
        tx: &TransactionWithId,
        _tx_block_index: usize,
        ctx: &mut IndexingContext,
    ) -> Result<(), Error> {
        let TransactionWithId { tx, .. } = tx;

        let mut seen_scripts = HashSet::new();

        if !tx.is_coinbase() {
            for input in &tx.input {
                let txo_ref = input.previous_output.into();

                let utxo = ctx
                    .resolve_input(&txo_ref)
                    .ok_or_else(|| Error::missing_utxo(txo_ref))?;

                seen_scripts.insert(utxo.script.clone());
            }
        }

        for output in &tx.output {
            seen_scripts.insert(output.script_pubkey.as_bytes().to_vec());
        }

        for script in seen_scripts {
            task.increment::<TxCountByAddressKV>(script, 1)?;
        }

        Ok(())
    }
}
