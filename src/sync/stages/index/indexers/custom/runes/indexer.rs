use std::collections::HashMap;

use crate::error::Error;
use crate::storage::encdec::Decode;
use crate::storage::kv_store::IndexingTask;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::id::ProcessTransaction;
use crate::sync::stages::index::indexers::types::{TxoRef, Utxo};
use crate::sync::stages::index::worker::context::IndexingContext;
use crate::sync::stages::{BlockHeight, TransactionWithId};
use bitcoin::Txid;
use bitcoin::{Network, ScriptBuf, Transaction, hashes::Hash};
use ordinals::{Artifact, Edict, Etching, Height, Rune, RuneId, Runestone};
use serde::Deserialize;

use super::tables::{
    RuneIdByNameKV, RuneInfo, RuneInfoByIdKV, RuneMintsByIdKV, RuneTerms, RuneUtxosByScriptKV,
    RuneUtxosByScriptKey, UtxoRunes,
};

// Import structures needed for optional rune operation logging
use super::tables::{RuneActivityByTxKV, RuneActivityByTxKey, RuneBalanceChange};

/// Internal key used for aggregating rune totals per script during a transaction.
#[derive(Hash, Eq, PartialEq, Clone, Debug, PartialOrd, Ord)]
struct AddressRuneKey {
    script: Vec<u8>,
    rune_id: RuneId,
}

/// Tracks rune activity for a specific address-rune combination during a transaction.
/// `sent` is aggregated across all inputs for this address+rune within the tx.
/// `received` is a map of `vout -> amount` so we can record multiple outputs
/// that credit the same rune to the same address in one transaction.
#[derive(Default, Debug)]
struct RuneActivity {
    sent: u128,
    received: Vec<(u32, u128)>,
}

pub struct RunesIndexer {
    start_height: u64,
    /// Enable indexing rune activity for each tx (RuneActivityByTxKV)
    index_activity: bool,
    /// Optionally override the default confirmations required for Rune commitment
    commitment_confirmations: Option<u64>,
}

impl RunesIndexer {
    pub fn new(config: RunesIndexerConfig) -> Result<Self, Error> {
        let start_height = config.start_height;
        let index_activity = config.index_activity;
        let commitment_confirmations = config.commitment_confirmations;

        Ok(Self {
            start_height,
            index_activity,
            commitment_confirmations,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RunesIndexerConfig {
    #[serde(default)]
    pub start_height: u64,
    /// Enable indexing rune activity for each tx (RuneActivityByTxKV)
    #[serde(default)]
    pub index_activity: bool,
    /// Optionally override the default confirmations required for Rune commitment
    commitment_confirmations: Option<u64>,
}

impl ProcessTransaction for RunesIndexer {
    fn process_tx(
        &self,
        task: &mut IndexingTask,
        tx: &TransactionWithId,
        tx_block_index: usize,
        ctx: &mut IndexingContext,
    ) -> Result<(), Error> {
        let TransactionWithId { tx, tx_id } = tx;
        let height = ctx.block_height();

        if height < self.start_height {
            return Ok(());
        }

        let artifact = Runestone::decipher(tx);

        let mut unallocated = unallocated(task, tx, ctx.resolver())?;

        let mut allocated: Vec<HashMap<RuneId, u128>> = vec![HashMap::new(); tx.output.len()];

        if let Some(artifact) = &artifact {
            if let Some(id) = artifact.mint() {
                if let Some(amount) = mint(task, id, height)? {
                    // debug!("minted rune: {height}:{tx_index} {:?}:{}", id, amount);

                    *unallocated.entry(id).or_default() += amount;
                }
            }

            let etched = etched(
                task,
                ctx.resolver(),
                tx_block_index,
                tx,
                artifact,
                height,
                ctx.network(),
                self.commitment_confirmations,
            )?;

            if let Artifact::Runestone(runestone) = artifact {
                if let Some((id, ..)) = etched {
                    *unallocated.entry(id).or_default() +=
                        runestone.etching.unwrap().premine.unwrap_or_default();
                }

                for Edict { id, amount, output } in runestone.edicts.iter().copied() {
                    // edicts with output values greater than the number of outputs
                    // should never be produced by the edict parser
                    let output = usize::try_from(output).unwrap();
                    assert!(output <= tx.output.len());

                    let id = if id == RuneId::default() {
                        let Some((id, ..)) = etched else {
                            continue;
                        };

                        id
                    } else {
                        id
                    };

                    let Some(balance) = unallocated.get_mut(&id) else {
                        continue;
                    };

                    let mut allocate = |balance: &mut u128, amount: u128, output: usize| {
                        if amount > 0 {
                            *balance -= amount;
                            *allocated[output].entry(id).or_default() += amount;
                        }
                    };

                    if output == tx.output.len() {
                        // find non-OP_RETURN outputs
                        let destinations = tx
                            .output
                            .iter()
                            .enumerate()
                            .filter_map(|(output, tx_out)| {
                                (!tx_out.script_pubkey.is_op_return()).then_some(output)
                            })
                            .collect::<Vec<usize>>();

                        if amount == 0 {
                            if !destinations.is_empty() {
                                // if amount is zero, divide balance between eligible outputs
                                let amount = *balance / destinations.len() as u128;
                                let remainder =
                                    usize::try_from(*balance % destinations.len() as u128).unwrap();

                                for (i, output) in destinations.iter().enumerate() {
                                    allocate(
                                        balance,
                                        if i < remainder { amount + 1 } else { amount },
                                        *output,
                                    );
                                }
                            }
                        } else {
                            // if amount is non-zero, distribute amount to eligible outputs
                            for output in destinations {
                                allocate(balance, amount.min(*balance), output);
                            }
                        }
                    } else {
                        // Get the allocatable amount
                        let amount = if amount == 0 {
                            *balance
                        } else {
                            amount.min(*balance)
                        };

                        allocate(balance, amount, output);
                    }
                }
            }

            if let Some((id, rune)) = etched {
                create_rune_entry(task, artifact, tx_id, id, rune, height)?;
            }
        }

        if let Some(Artifact::Cenotaph(_)) = artifact {
            for (_id, _balance) in unallocated {
                // if cenotaph, all unallocated runes burned
            }
        } else {
            let pointer = artifact
                .map(|artifact| match artifact {
                    Artifact::Runestone(runestone) => runestone.pointer,
                    Artifact::Cenotaph(_) => unreachable!(),
                })
                .unwrap_or_default();

            // assign all un-allocated runes to the default output, or the first non
            // OP_RETURN output if there is no default, or if the default output is
            // too large
            if let Some(vout) = pointer
                .map(|pointer| pointer as usize)
                .inspect(|&pointer| assert!(pointer < tx.output.len()))
                .or_else(|| {
                    tx.output
                        .iter()
                        .enumerate()
                        .find(|(_vout, tx_out)| !tx_out.script_pubkey.is_op_return())
                        .map(|(vout, _tx_out)| vout)
                })
            {
                for (id, balance) in unallocated {
                    if balance > 0 {
                        *allocated[vout].entry(id).or_default() += balance;
                    }
                }
            } else {
                for (_id, _balance) in unallocated {
                    // anything remaining in unallocated is burned (check bal greater than 0)
                }
            }
        }

        // allocated contains runes for each output (may be empty)
        // We'll also record rune operations (movements) for logging
        let mut seq_counter: u16 = 0;

        // Track all rune activity (sent/received) per address-rune combination
        let mut rune_activity: HashMap<AddressRuneKey, RuneActivity> = HashMap::new();

        if self.index_activity {
            collect_input_totals(tx, ctx, &mut rune_activity)?;
        }

        for ((output_index, output), output_runes) in tx.output.iter().enumerate().zip(allocated) {
            let txo_ref = TxoRef {
                tx_hash: tx_id.to_byte_array(),
                txo_index: output_index.try_into().expect("output index u32 overflow"),
            };

            if !output_runes.is_empty() {
                // attach runes to utxo metadata so we can resolve them
                let output_runes_vec: UtxoRunes = output_runes.into_iter().collect();
                ctx.attach_utxo_metadata(
                    txo_ref,
                    TransactionIndexer::Runes,
                    output_runes_vec.clone(),
                );

                // TODO: helper
                // add kv for address utxo containing runes
                let script_pubkey = output.script_pubkey.as_bytes().to_vec();
                let key = RuneUtxosByScriptKey {
                    script: script_pubkey,
                    produced_height: height,
                    txo_ref,
                };

                task.set::<RuneUtxosByScriptKV>(key, ())?;

                // Record rune movements for this output
                if self.index_activity {
                    // Update received totals
                    for (rune_id, amount) in &output_runes_vec {
                        let key = AddressRuneKey {
                            script: output.script_pubkey.as_bytes().to_vec(),
                            rune_id: *rune_id,
                        };
                        rune_activity
                            .entry(key)
                            .or_default()
                            .received
                            .push((output_index as u32, *amount));
                    }
                }
            }
        }

        // After processing all outputs, emit balance change records if logging is enabled.
        if self.index_activity {
            log_rune_balance_changes(task, &mut seq_counter, tx_id.to_byte_array(), rune_activity)?;
        }

        Ok(())
    }
}

/// Discover runes in transaction inputs
fn unallocated(
    task: &mut IndexingTask,
    tx: &Transaction,
    resolver: &HashMap<TxoRef, Utxo>,
) -> Result<HashMap<RuneId, u128>, Error> {
    // map of rune ID to un-allocated balance of that rune
    let mut unallocated: HashMap<RuneId, u128> = HashMap::new();

    // increment unallocated runes with the runes in tx inputs
    for input in &tx.input {
        // skip coinbase input
        if !input.previous_output.is_null() {
            let txo_ref = input.previous_output.into();

            let utxo = resolver.get(&txo_ref).expect("todo");

            // TODO: helper
            let runes = match utxo.extended.get(&TransactionIndexer::Runes) {
                Some(raw_utxo_metadata) => UtxoRunes::decode_all(raw_utxo_metadata)?,
                None => vec![],
            };

            // delete kv for this utxo which contains runes now it is being consumed
            if !runes.is_empty() {
                // TODO helper
                let key = RuneUtxosByScriptKey {
                    script: utxo.script.clone(),
                    produced_height: utxo.height,
                    txo_ref,
                };

                task.delete::<RuneUtxosByScriptKV>(key)?;
            };

            for (id, balance) in runes {
                *unallocated.entry(id).or_default() += balance;
            }
        }
    }

    Ok(unallocated)
}

fn mint(task: &mut IndexingTask, id: RuneId, height: BlockHeight) -> Result<Option<u128>, Error> {
    let Some(terms) = task.get::<RuneInfoByIdKV>(&id)?.and_then(|x| x.terms) else {
        return Ok(None);
    };

    if let Some(start) = terms.start_height {
        if height < start {
            return Ok(None);
        }
    }

    if let Some(end) = terms.end_height {
        if height >= end {
            return Ok(None);
        }
    }

    let cap = terms.cap.unwrap_or_default();

    let current_mints = task.get::<RuneMintsByIdKV>(&id)?.unwrap_or_default();

    if current_mints >= cap {
        return Ok(None);
    }

    let new_mints = current_mints.checked_add(1).expect("mints overflow");

    task.set::<RuneMintsByIdKV>(id, new_mints)?;

    Ok(Some(terms.amount.unwrap_or_default()))
}

fn tx_commits_to_rune(
    tx: &Transaction,
    rune: Rune,
    height: BlockHeight,
    resolver: &HashMap<TxoRef, Utxo>,
    confirmations_override: Option<u64>,
) -> Result<bool, Error> {
    let commitment = rune.commitment();

    let required_confirmations =
        confirmations_override.unwrap_or(Runestone::COMMIT_CONFIRMATIONS.into());

    for input in &tx.input {
        // extracting a tapscript does not indicate that the input being spent
        // was actually a taproot output. this is checked below, when we load the
        // output's entry from the database
        let Some(tapscript) = input.witness.tapscript() else {
            continue;
        };

        for instruction in tapscript.instructions() {
            // ignore errors, since the extracted script may not be valid
            let Ok(instruction) = instruction else {
                break;
            };

            let Some(pushbytes) = instruction.push_bytes() else {
                continue;
            };

            if pushbytes.as_bytes() != commitment {
                continue;
            }

            let txo_ref = input.previous_output.into();

            let utxo = resolver
                .get(&txo_ref)
                .expect("missing txo resolver in rune commit"); // TODO

            // check taproot
            if !ScriptBuf::from_bytes(utxo.script.clone())
                .as_script()
                .is_p2tr()
            {
                continue;
            }

            let commit_tx_height = utxo.height;

            let confirmations = height
                .checked_sub(commit_tx_height)
                .expect("rune commit underflow")
                .checked_add(1)
                .expect("rune commit overflow");

            if confirmations >= required_confirmations {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn etched(
    task: &mut IndexingTask,
    resolver: &HashMap<TxoRef, Utxo>,
    tx_index: usize,
    tx: &Transaction,
    artifact: &Artifact,
    height: BlockHeight,
    network: Network,
    confirmations_override: Option<u64>,
) -> Result<Option<(RuneId, Rune)>, Error> {
    let tx_index = tx_index.try_into().expect("tx index u32 overflow");

    let rune = match artifact {
        Artifact::Runestone(runestone) => match runestone.etching {
            Some(etching) => etching.rune,
            None => return Ok(None),
        },
        Artifact::Cenotaph(cenotaph) => match cenotaph.etching {
            Some(rune) => Some(rune),
            None => return Ok(None),
        },
    };

    let minimum = Rune::minimum_at_height(
        network,
        Height(height.try_into().expect("height u32 overflow")),
    );

    let rune = if let Some(rune) = rune {
        if rune < minimum
            || rune.is_reserved()
            || task.get::<RuneIdByNameKV>(&rune.n())?.is_some()
            || !tx_commits_to_rune(tx, rune, height, resolver, confirmations_override)?
        {
            return Ok(None);
        }
        rune
    } else {
        Rune::reserved(height, tx_index)
    };

    Ok(Some((
        RuneId {
            block: height,
            tx: tx_index,
        },
        rune,
    )))
}

fn create_rune_entry(
    task: &mut IndexingTask,
    artifact: &Artifact,
    tx_id: &Txid,
    id: RuneId,
    rune: Rune,
    height: BlockHeight,
) -> Result<(), Error> {
    // insert new name to id mapping
    task.set::<RuneIdByNameKV>(rune.n(), id)?;

    let info = match artifact {
        Artifact::Cenotaph(_) => RuneInfo {
            name: rune.0,
            terms: None,
            symbol: None,
            divisibility: 0,
            etching_height: height,
            etching_tx: tx_id.to_byte_array(),
            premine: 0,
            spacers: 0,
        },
        Artifact::Runestone(Runestone { etching, .. }) => {
            let Etching {
                terms,
                premine,
                divisibility,
                spacers,
                symbol,
                ..
            } = etching.unwrap();

            if let Some(terms) = terms {
                let amount = terms.amount;
                let cap = terms.cap;

                let relative_start = terms.offset.0.map(|offset| height.saturating_add(offset));

                let absolute_start = terms.height.0;

                let start = relative_start
                    .zip(absolute_start)
                    .map(|(relative, absolute)| relative.max(absolute))
                    .or(relative_start)
                    .or(absolute_start);

                let relative_end = terms.offset.1.map(|offset| height.saturating_add(offset));

                let absolute_end = terms.height.1;

                let end = relative_end
                    .zip(absolute_end)
                    .map(|(relative, absolute)| relative.min(absolute))
                    .or(relative_end)
                    .or(absolute_end);

                RuneInfo {
                    name: rune.0,
                    terms: Some(RuneTerms {
                        amount,
                        cap,
                        start_height: start,
                        end_height: end,
                    }),
                    symbol: symbol.map(|x| x.into()),
                    divisibility: divisibility.unwrap_or(0),
                    etching_height: height,
                    etching_tx: tx_id.to_byte_array(),
                    premine: premine.unwrap_or(0),
                    spacers: spacers.unwrap_or(0),
                }
            } else {
                RuneInfo {
                    name: rune.0,
                    terms: None,
                    symbol: symbol.map(|x| x.into()),
                    divisibility: divisibility.unwrap_or(0),
                    etching_height: height,
                    etching_tx: tx_id.to_byte_array(),
                    premine: premine.unwrap_or(0),
                    spacers: spacers.unwrap_or(0),
                }
            }
        }
    };

    // insert new id to rune terms mapping
    task.set::<RuneInfoByIdKV>(id, info)?;

    Ok(())
}

/// Scan all inputs and aggregate total amount of each rune spent per script.
fn collect_input_totals(
    tx: &Transaction,
    ctx: &IndexingContext,
    rune_activity: &mut HashMap<AddressRuneKey, RuneActivity>,
) -> Result<(), Error> {
    if tx.is_coinbase() {
        return Ok(());
    }

    for input in &tx.input {
        if input.previous_output.is_null() {
            continue;
        }

        let txo_ref = input.previous_output.into();
        let Some(utxo) = ctx.resolve_input(&txo_ref) else {
            continue;
        }; // orphaned input
        let Some(raw) = utxo.extended.get(&TransactionIndexer::Runes) else {
            continue;
        };

        for (id, qty) in UtxoRunes::decode_all(raw)? {
            let key = AddressRuneKey {
                script: utxo.script.clone(),
                rune_id: id,
            };
            rune_activity.entry(key).or_default().sent += qty;
        }
    }

    Ok(())
}

/// After aggregating sent and received totals, emit `RuneBalanceChange` records.
fn log_rune_balance_changes(
    task: &mut IndexingTask,
    seq_counter: &mut u16,
    tx_hash: [u8; 32],
    rune_activity: HashMap<AddressRuneKey, RuneActivity>,
) -> Result<(), Error> {
    for (key, activity) in rune_activity {
        // Emit spend record if any
        if activity.sent > 0 {
            let change = RuneBalanceChange {
                rune_id: key.rune_id,
                script: key.script.clone(),
                sent: activity.sent,
                received: 0,
                output_index: None,
            };

            let key_tx = RuneActivityByTxKey {
                tx_hash,
                seq: *seq_counter,
            };
            *seq_counter = seq_counter.checked_add(1).expect("seq overflow");
            task.set::<RuneActivityByTxKV>(key_tx, change)?;
        }

        // Emit separate records for each received output
        for (vout, amount) in activity.received {
            if amount == 0 {
                continue;
            }

            let change = RuneBalanceChange {
                rune_id: key.rune_id,
                script: key.script.clone(),
                sent: 0,
                received: amount,
                output_index: Some(vout),
            };

            let key_tx = RuneActivityByTxKey {
                tx_hash,
                seq: *seq_counter,
            };
            *seq_counter = seq_counter.checked_add(1).expect("seq overflow");
            task.set::<RuneActivityByTxKV>(key_tx, change)?;
        }
    }

    Ok(())
}
