use bitcoin::{BlockHash, Network, hashes::Hash};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    error::Error,
    storage::{encdec::Encode, kv_store::IndexingTask},
    sync::{
        self,
        stages::{
            BlockHeight, BlockTxs, Point, TransactionWithId,
            index::indexers::{
                core::utxo_by_txo_ref::{ResolvedUtxos, UtxoByTxoRefKV, UtxoCache},
                custom::TransactionIndexer,
                types::{ExtendedUtxoData, TxoRef, Utxo},
            },
        },
    },
};

// A context object that encapsulates all the data needed for indexing a transaction
pub struct IndexingContext {
    point: Point,
    resolver: FxHashMap<TxoRef, Utxo>,
    utxo_metadata: FxHashMap<TxoRef, ExtendedUtxoData>,
    chained_txos: FxHashSet<TxoRef>,
    network: Network,
    stats: Stats,
}

// Public methods available to custom indexers
impl IndexingContext {
    pub fn block_height(&self) -> BlockHeight {
        self.point.height
    }

    pub fn block_hash(&self) -> BlockHash {
        self.point.hash
    }

    pub fn resolver(&self) -> &FxHashMap<TxoRef, Utxo> {
        &self.resolver
    }

    pub fn network(&self) -> Network {
        self.network
    }

    // Given an input TxoRef return the UTxO information (amount, script, etc)
    pub fn resolve_input(&self, txo_ref: &TxoRef) -> Option<&Utxo> {
        self.resolver.get(txo_ref)
    }

    // Method for indexers to attach metadata to a specific output
    pub fn attach_utxo_metadata<E: Encode>(
        &mut self,
        txo_ref: TxoRef,
        indexer: TransactionIndexer,
        data: E,
    ) {
        self.utxo_metadata
            .entry(txo_ref)
            .or_default()
            .insert(indexer, data.encode());
    }
}

// Internal methods only used by the stage worker
impl IndexingContext {
    pub(super) fn new(
        task: &mut IndexingTask,
        txs: &BlockTxs,
        point: Point,
        network: sync::Network,
        utxo_cache: &mut Option<UtxoCache>,
    ) -> Result<Self, Error> {
        let ResolvedUtxos {
            resolver,
            chained_txos,
            cache_hits,
        } = UtxoByTxoRefKV::resolve_inputs(task, txs, utxo_cache)?;

        let network = match network {
            sync::Network::Mainnet => Network::Bitcoin,
            sync::Network::Testnet4 => Network::Testnet4,
            sync::Network::Regtest => Network::Regtest,
        };

        let stats = Stats {
            resolved: resolver.len(),
            cache_hits,
        };

        Ok(Self {
            point,
            resolver,
            utxo_metadata: Default::default(),
            chained_txos,
            network,
            stats,
        })
    }

    pub(super) fn update_utxo_set(
        &mut self,
        task: &mut IndexingTask,
        tx: &TransactionWithId,
        utxo_cache: &mut Option<UtxoCache>,
    ) -> Result<usize, Error> {
        let mut outs_written = 0;

        // remove consumed utxos from resolver and storage
        for input in &tx.tx.input {
            let txo_ref = input.previous_output.into();

            // dont remove from cache if mempool block
            if !task.mempool() {
                self.resolver.remove(&txo_ref);
            }

            task.delete::<UtxoByTxoRefKV>(txo_ref)?;
        }

        // for produced utxos, add those which are consumed later in the block to the resolver, and
        // the rest to storage
        for (output_index, output) in tx.tx.output.iter().enumerate() {
            // don't write unspendables
            if !output.script_pubkey.is_op_return() {
                let txo_ref = TxoRef {
                    tx_hash: tx.tx_id.to_byte_array(),
                    txo_index: output_index as u32,
                };

                let utxo = Utxo {
                    satoshis: output.value.to_sat(),
                    script: output.script_pubkey.to_bytes(),
                    height: self.block_height(),
                    extended: self.utxo_metadata.remove(&txo_ref).unwrap_or_default(),
                };

                if self.chained_txos.contains(&txo_ref) {
                    self.resolver.insert(txo_ref, utxo);
                } else if let Some(c) = utxo_cache.as_mut() {
                    // dont insert mempool outputs into cache
                    if !task.mempool() {
                        c.insert(txo_ref, utxo.clone());
                    }

                    outs_written += 1;
                    task.set::<UtxoByTxoRefKV>(txo_ref, utxo)?;
                } else {
                    outs_written += 1;
                    task.set::<UtxoByTxoRefKV>(txo_ref, utxo)?;
                }
            }
        }

        if let Some(c) = utxo_cache.as_mut() {
            c.wipe_if_mem_usage_high();
        }

        Ok(outs_written)
    }

    pub(super) fn update_point(&mut self, point: Point) {
        self.point = point
    }

    pub(super) fn stats(&self) -> Stats {
        self.stats
    }
}

#[derive(Clone, Copy)]
pub struct Stats {
    resolved: usize,
    cache_hits: usize,
}

impl Stats {
    pub fn log(&self) -> String {
        if self.resolved == 0 {
            format!("{}/{}(N/A%)", self.cache_hits, self.resolved)
        } else {
            format!(
                "{}/{}({}%)",
                self.cache_hits,
                self.resolved,
                ((self.cache_hits as f64 / self.resolved as f64) * 100.0) as u64
            )
        }
    }
}
