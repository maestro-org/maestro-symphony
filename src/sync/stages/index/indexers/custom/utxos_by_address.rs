use super::id::ProcessTransaction;
use crate::error::Error;
use crate::storage::{kv_store::Task, table::IndexerTable};
use crate::sync::stages::TransactionWithId;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::types::TxoRef;
use crate::sync::stages::index::worker::context::IndexingContext;
use crate::{define_indexer_table, sync::stages::index::indexers::types::ScriptPubKey};
use bitcoin::hashes::Hash;
use maestro_symphony_macros::{Decode, Encode};

// --- storage

define_indexer_table! {
    name: UtxosByAddressKV,
    key_type: UtxosByAddressKey,
    value_type: (), // fetch data from UtxoByTxoRef
    indexer: TransactionIndexer::UtxosByAddress,
    table: 0
}

#[derive(Encode, Decode)]
pub struct UtxosByAddressKey {
    pub script: ScriptPubKey,
    pub produced_height: u64,
    pub txo_ref: TxoRef,
}

// --- indexer

pub struct UtxosByAddressIndexer;

impl UtxosByAddressIndexer {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessTransaction for UtxosByAddressIndexer {
    fn process_tx(
        &self,
        task: &mut Task,
        tx: &TransactionWithId,
        _tx_block_index: usize,
        ctx: &mut IndexingContext,
    ) -> Result<(), Error> {
        let TransactionWithId { tx, tx_id } = tx;

        if !tx.is_coinbase() {
            for input in tx.input.iter() {
                let txo_ref = input.previous_output.into();
                let utxo = ctx.resolve_input(&txo_ref).unwrap();

                // delete kv for consumed utxo

                let key = UtxosByAddressKey {
                    script: utxo.script.clone(),
                    produced_height: utxo.height,
                    txo_ref,
                };

                task.delete::<UtxosByAddressKV>(key)?;
            }
        }

        for (output_index, output) in tx.output.iter().enumerate() {
            let txo_ref = TxoRef {
                tx_hash: tx_id.to_byte_array(),
                txo_index: output_index as u32,
            };

            let script = output.script_pubkey.as_bytes().to_vec();

            let key = UtxosByAddressKey {
                script: script,
                produced_height: ctx.block_height(),
                txo_ref,
            };

            task.set::<UtxosByAddressKV>(key, ())?;
        }

        Ok(())
    }
}
