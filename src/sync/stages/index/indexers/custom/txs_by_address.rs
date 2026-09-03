use std::collections::HashMap;

use super::id::ProcessTransaction;
use crate::define_indexer_table;
use crate::error::Error;
use crate::storage::kv_store::IndexingTask;
use crate::storage::table::IndexerTable;
use crate::sync::stages::TransactionWithId;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::types::ScriptPubKey;
use crate::sync::stages::index::worker::context::IndexingContext;
use bitcoin::hashes::Hash;
use maestro_symphony_macros::{Decode, Encode};

// --- storage

define_indexer_table! {
    name: TxsByAddressKV,
    key_type: TxsByAddressKey,
    value_type: TxAddressRoles,
    indexer: TransactionIndexer::TxsByAddress,
    table: 0
}

/// Composite key ordered so that a per-script prefix scan yields that script's
/// transactions in chain order (height, then intra-block position).
#[derive(Encode, Decode, Debug, PartialEq)]
pub struct TxsByAddressKey {
    pub script: ScriptPubKey,
    pub height: u64,
    pub tx_index: u32,
    pub tx_hash: [u8; 32],
}

/// How the script participated in the transaction (controlled an input, an
/// output, or both).
#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq)]
pub struct TxAddressRoles {
    pub input: bool,
    pub output: bool,
}

// --- indexer

/// Indexes, per script pubkey, every transaction in which the script
/// controlled an input or an output. History is append-only: unlike
/// UtxosByAddress nothing is deleted when outputs are spent, so this table
/// grows with chain history.
pub struct TxsByAddressIndexer;

impl TxsByAddressIndexer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TxsByAddressIndexer {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessTransaction for TxsByAddressIndexer {
    fn process_tx(
        &self,
        task: &mut IndexingTask,
        tx: &TransactionWithId,
        tx_block_index: usize,
        ctx: &mut IndexingContext,
    ) -> Result<(), Error> {
        let TransactionWithId { tx, tx_id } = tx;

        let mut seen_scripts: HashMap<ScriptPubKey, TxAddressRoles> = HashMap::new();

        if !tx.is_coinbase() {
            for input in &tx.input {
                let txo_ref = input.previous_output.into();

                let utxo = ctx
                    .resolve_input(&txo_ref)
                    .ok_or_else(|| Error::missing_utxo(txo_ref))?;

                seen_scripts
                    .entry(utxo.script.clone())
                    .or_insert(TxAddressRoles {
                        input: false,
                        output: false,
                    })
                    .input = true;
            }
        }

        for output in &tx.output {
            seen_scripts
                .entry(output.script_pubkey.as_bytes().to_vec())
                .or_insert(TxAddressRoles {
                    input: false,
                    output: false,
                })
                .output = true;
        }

        for (script, roles) in seen_scripts {
            let key = TxsByAddressKey {
                script,
                height: ctx.block_height(),
                tx_index: tx_block_index as u32,
                tx_hash: tx_id.to_byte_array(),
            };

            task.set::<TxsByAddressKV>(key, roles)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::encdec::{Decode, Encode};
    use crate::storage::table::Table;

    fn key(script: &[u8], height: u64, tx_index: u32, tx_hash: u8) -> TxsByAddressKey {
        TxsByAddressKey {
            script: script.to_vec(),
            height,
            tx_index,
            tx_hash: [tx_hash; 32],
        }
    }

    #[test]
    fn key_roundtrips() {
        let original = key(&[0x00, 0x14, 0xab], 150_813, 42, 7);
        let decoded = TxsByAddressKey::decode_all(&original.encode()).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn encoded_keys_sort_by_height_then_tx_index() {
        let script = [0x00, 0x14, 0xab];
        let mut encoded: Vec<Vec<u8>> = [
            key(&script, 2, 0, 1),
            key(&script, 10, 5, 2),
            key(&script, 10, 40, 3),
            key(&script, 300, 0, 4),
            key(&script, 70_000, 1, 5),
            key(&script, u64::MAX, 0, 6),
        ]
        .iter()
        .map(TxsByAddressKV::encode_key)
        .collect();

        let expected = encoded.clone();
        encoded.sort();
        assert_eq!(encoded, expected);
    }

    #[test]
    fn scripts_partition_the_keyspace() {
        let a = TxsByAddressKV::encode_key(&key(&[0x00, 0x14, 0x01], u64::MAX, u32::MAX, 0xff));
        let b = TxsByAddressKV::encode_key(&key(&[0x00, 0x14, 0x02], 0, 0, 0x00));
        assert!(a < b);
    }

    #[test]
    fn roles_roundtrip() {
        for (input, output) in [(true, false), (false, true), (true, true)] {
            let roles = TxAddressRoles { input, output };
            assert_eq!(TxAddressRoles::decode_all(&roles.encode()).unwrap(), roles);
        }
    }
}
