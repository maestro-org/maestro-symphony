use crate::error::Error;
use crate::storage::encdec::Decode;
use crate::storage::kv_store::IndexingTask;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::id::ProcessTransaction;
use crate::sync::stages::index::indexers::types::TxoRef;
use crate::sync::stages::index::worker::context::IndexingContext;
use crate::sync::stages::TransactionWithId;
use bitcoin::hashes::Hash;
use charms_lib::extract_and_verify_spell;
use serde::Deserialize;
use super::tables::{
    CharmsUtxosByAppKV, CharmsUtxosByAppKey, CharmsUtxosByScriptKV, CharmsUtxosByScriptKey,
    UtxoCharms,
};

#[derive(Clone, Debug, Deserialize)]
pub struct CharmsIndexerConfig {
    #[serde(default)]
    pub start_height: u64,
}

pub struct CharmsIndexer {
    start_height: u64,
}

impl CharmsIndexer {
    pub fn new(c: CharmsIndexerConfig) -> Self {
        CharmsIndexer {
            start_height: c.start_height,
        }
    }
}

impl ProcessTransaction for CharmsIndexer {
    fn process_tx(
        &self,
        task: &mut IndexingTask,
        tx: &TransactionWithId,
        _tx_block_index: usize,
        ctx: &mut IndexingContext,
    ) -> Result<(), Error> {
        let TransactionWithId { tx, tx_id } = tx;
        let height = ctx.block_height();

        if height < self.start_height {
            return Ok(());
        }

        // Always clean up consumed charm UTXOs from index tables, even if this tx has no spell
        // (spending a UTXO with charms without a valid spell destroys those charms)
        for input in &tx.input {
            if !input.previous_output.is_null() {
                let txo_ref: TxoRef = input.previous_output.into();

                if let Some(utxo) = ctx.resolve_input(&txo_ref) {
                    if let Some(raw) = utxo.extended.get(&TransactionIndexer::Charms) {
                        let utxo_charms = UtxoCharms::decode_all(raw)?;

                        if !utxo_charms.is_empty() {
                            task.delete::<CharmsUtxosByScriptKV>(CharmsUtxosByScriptKey {
                                script: utxo.script.clone(),
                                produced_height: utxo.height,
                                txo_ref,
                            })?;

                            for (app_bytes, _) in &utxo_charms {
                                task.delete::<CharmsUtxosByAppKV>(CharmsUtxosByAppKey {
                                    app: app_bytes.clone(),
                                    produced_height: utxo.height,
                                    txo_ref,
                                })?;
                            }
                        }
                    }
                }
            }
        }

        // Only index non-mock spells for outputs
        let charms_tx =
            charms_lib::tx::Tx::Bitcoin(charms_lib::bitcoin_tx::BitcoinTx::Simple(tx.clone()));
        let spell = match extract_and_verify_spell(&charms_tx, false) {
            Ok(spell) => spell,
            Err(_) => return Ok(()), // No valid spell — inputs cleaned up, nothing to produce
        };

        // Resolve app indices: NormalizedCharms uses u32 indices into app_public_inputs keys
        let apps: Vec<charms_data::App> = spell.app_public_inputs.keys().cloned().collect();

        let beamed = spell.tx.beamed_outs.as_ref();

        // Process outputs
        for (output_index, (output, n_charms)) in
            tx.output.iter().zip(spell.tx.outs.iter()).enumerate()
        {
            let output_index_u32 = output_index as u32;

            // Skip beamed outputs — charms transferred to another chain
            if beamed.is_some_and(|b| b.contains_key(&output_index_u32)) {
                continue;
            }

            if n_charms.is_empty() {
                continue;
            }

            let txo_ref = TxoRef {
                tx_hash: tx_id.to_byte_array(),
                txo_index: output_index_u32,
            };

            // Convert NormalizedCharms to UtxoCharms (app string bytes, data CBOR bytes)
            let utxo_charms: UtxoCharms = n_charms
                .iter()
                .filter_map(|(&app_idx, data)| {
                    apps
                        .get(app_idx as usize)
                        .map(|app| (app.to_string().into_bytes(), data.bytes()))
                })
                .collect();

            ctx.attach_utxo_metadata(txo_ref, TransactionIndexer::Charms, utxo_charms.clone());

            let script_pubkey = output.script_pubkey.as_bytes().to_vec();
            task.set::<CharmsUtxosByScriptKV>(
                CharmsUtxosByScriptKey {
                    script: script_pubkey,
                    produced_height: height,
                    txo_ref,
                },
                (),
            )?;

            for (app_bytes, _) in &utxo_charms {
                task.set::<CharmsUtxosByAppKV>(
                    CharmsUtxosByAppKey {
                        app: app_bytes.clone(),
                        produced_height: height,
                        txo_ref,
                    },
                    (),
                )?;
            }
        }

        Ok(())
    }
}
