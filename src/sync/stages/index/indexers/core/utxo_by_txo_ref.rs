/// Represents a table that maps a transaction output reference (`TxoRef`) to an unspent transaction
/// output (`Utxo`).
///
/// This table is part of the core indexing system and is used to look up UTxOs by their transaction
/// hash and output index, so that when processing transactions we can find the UTxOs used as
/// inputs.
///
/// # Table Definition
/// - **Key**: [`TxoRef`] - A reference to a specific transaction output, consisting of a
///   transaction hash and an output index.
/// - **Value**: [`Utxo`] - The unspent transaction output, containing details such as the amount,
///   script, block height, and extended metadata.
/// - **Indexer**: [`CoreIndexer::UtxoByTxoRef`] - The indexer responsible for managing this table.
use bitcoin::hashes::Hash;
use mini_moka::sync::Cache;
use rustc_hash::{FxHashMap, FxHashSet};
use sysinfo::{Pid, System};
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::{
    define_core_table,
    error::Error,
    storage::{encdec::Encode, kv_store::IndexingTask, table::CoreTable},
    sync::stages::{
        BlockTxs,
        index::indexers::types::{TxoRef, Utxo},
    },
};

use super::CoreIndexer;

define_core_table! {
    name: UtxoByTxoRefKV,
    key_type: TxoRef,
    value_type: Utxo,
    indexer: CoreIndexer::UtxoByTxoRef
}

pub struct ResolvedUtxos {
    pub resolver: FxHashMap<TxoRef, Utxo>,
    pub chained_txos: FxHashSet<TxoRef>,
    pub cache_hits: usize,
}

impl UtxoByTxoRefKV {
    pub fn resolve_inputs(
        task: &IndexingTask,
        txs: &BlockTxs,
        cache: &mut Option<UtxoCache>,
    ) -> Result<ResolvedUtxos, Error> {
        let start = Instant::now();
        let mut cache_hits = 0;

        let tx_ids = txs
            .iter()
            .map(|x| x.tx_id.to_byte_array())
            .collect::<FxHashSet<_>>();

        // skip first tx if it is coinbase (mempool block txs dont have one)
        let skip = if txs
            .first()
            .map(|tx| tx.tx.is_coinbase())
            .unwrap_or_default()
        {
            1
        } else {
            0
        };

        let input_refs = txs
            .iter()
            .skip(skip) // skip coinbase if applicable
            .flat_map(|x| x.tx.input.iter())
            .map(|x| TxoRef {
                tx_hash: x.previous_output.txid.to_byte_array(),
                txo_index: x.previous_output.vout,
            })
            .collect::<FxHashSet<_>>();

        let total_inputs = input_refs.len();

        let mut resolver = FxHashMap::default();
        resolver.reserve(total_inputs);

        let mut fetch_from_db = Vec::with_capacity(total_inputs);

        // txos which are not found in db must be produced and consumed within the block ("chained")
        let mut chained_txos = FxHashSet::default();

        // first try resolve input utxos using cache
        for input_ref in input_refs {
            // ignore utxos produced in this block
            if tx_ids.contains(&input_ref.tx_hash) {
                chained_txos.insert(input_ref);
                continue;
            }

            // if the utxo is not found in the cache we need to fetch it from db
            if let Some(utxo) = cache.as_mut().and_then(|c| c.remove(&input_ref)) {
                cache_hits += 1;
                resolver.insert(input_ref, utxo);
            } else {
                fetch_from_db.push(input_ref);
            }
        }

        // then fetch the remaining from storage
        if !fetch_from_db.is_empty() {
            let fetched_utxos = task.multi_get::<Self>(fetch_from_db)?;

            for (txo_ref, maybe_utxo) in fetched_utxos {
                match maybe_utxo {
                    Some(u) => {
                        resolver.insert(txo_ref, u);
                    }
                    None => panic!("missing non chained utxo {txo_ref:?}"),
                }
            }
        }

        let not_chained = total_inputs - chained_txos.len();

        debug!(
            "finished resolving utxos in {:?} ({} total, {} required, {} not in cache)",
            start.elapsed(),
            total_inputs,
            not_chained,
            not_chained - cache_hits
        );

        Ok(ResolvedUtxos {
            resolver,
            chained_txos,
            cache_hits,
        })
    }
}

pub struct UtxoCache {
    cache: Cache<TxoRef, Utxo>,
    last_check: Instant,
    max_capacity: u64,
}

impl UtxoCache {
    pub fn new(max_size_bytes: u64) -> Self {
        Self {
            cache: Cache::builder()
                .weigher(|_key: &TxoRef, value: &Utxo| -> u32 {
                    // Get byte length of serialized UTxO (increased to be conservative)
                    (value.encode().len() as u32 + 36) * 6
                })
                .max_capacity(max_size_bytes)
                .build(),
            last_check: Instant::now(),
            max_capacity: max_size_bytes,
        }
    }

    pub fn get(&self, txo_ref: &TxoRef) -> Option<Utxo> {
        self.cache.get(txo_ref)
    }

    pub fn insert(&self, txo_ref: TxoRef, utxo: Utxo) {
        self.cache.insert(txo_ref, utxo);
    }

    pub fn remove(&self, txo_ref: &TxoRef) -> Option<Utxo> {
        let res = self.cache.get(txo_ref);
        self.cache.invalidate(txo_ref);

        res
    }

    pub fn contains_key(&self, txo_ref: &TxoRef) -> bool {
        self.cache.contains_key(txo_ref)
    }

    pub fn log(&self) -> String {
        format!(
            "entries={} weight={}",
            self.cache.entry_count(),
            self.cache.weighted_size(),
        )
    }

    pub fn wipe_if_mem_usage_high(&mut self) {
        // Only check memory usage every 300 seconds to avoid overhead
        let now = Instant::now();
        if now.duration_since(self.last_check).as_secs() < 300 {
            return;
        }
        self.last_check = now;

        let mut system = System::new_all();

        system.refresh_memory();

        let total_memory = system
            .cgroup_limits()
            .map(|x| x.total_memory)
            .unwrap_or_else(|| system.total_memory());

        let pid = std::process::id();
        let process = system.process(Pid::from_u32(pid)).unwrap();
        let app_mem = process.memory();

        if total_memory == 0 {
            warn!("Unable to determine memory usage, skipping cache wipe check");
            return;
        }

        let usage_percentage = (app_mem as f64 / total_memory as f64) * 100.0;

        if usage_percentage >= 90.0 {
            let entries_before = self.cache.entry_count();
            let weight_before = self.cache.weighted_size();

            self.cache = Cache::builder()
                .weigher(|_key: &TxoRef, value: &Utxo| -> u32 {
                    // Get byte length of serialized UTxO (increased to be conservative)
                    (value.encode().len() as u32 + 36) * 6
                })
                .max_capacity(self.max_capacity)
                .build();

            warn!(
                "Memory usage at {:.1}% ({} MB used / {} MB total), wiped UTXO cache ({} entries, {} weight)",
                usage_percentage,
                app_mem / 1024 / 1024,
                total_memory / 1024 / 1024,
                entries_before,
                weight_before
            );
        }
    }
}
