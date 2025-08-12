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
use std::collections::{HashMap, HashSet};

use bitcoin::hashes::Hash;
use mini_moka::sync::Cache;
use tokio::time::Instant;
use tracing::debug;

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
    pub resolver: HashMap<TxoRef, Utxo>,
    pub chained_txos: HashSet<TxoRef>,
}

impl UtxoByTxoRefKV {
    pub fn resolve_inputs(
        task: &IndexingTask,
        txs: &BlockTxs,
        cache: &mut Option<UtxoCache>,
        partial_sync: bool,
    ) -> Result<ResolvedUtxos, Error> {
        let start = Instant::now();
        let mut cache_hits = 0;

        let tx_ids = txs
            .iter()
            .map(|x| x.tx_id.to_byte_array())
            .collect::<HashSet<_>>();

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
            .collect::<HashSet<_>>();

        let total_inputs = input_refs.len();

        let mut resolver = HashMap::with_capacity(total_inputs);

        let mut fetch_from_db = Vec::with_capacity(total_inputs);

        // txos which are not found in db must be produced and consumed within the block ("chained")
        let mut chained_txos = HashSet::new();

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
                    None => {
                        if !partial_sync {
                            panic!("missing non chained utxo {txo_ref:?}")
                        }
                    }
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
        })
    }
}

pub struct UtxoCache {
    cache: Cache<TxoRef, Utxo>,
}

impl UtxoCache {
    pub fn new(max_size_bytes: u64) -> Self {
        Self {
            cache: Cache::builder()
                .weigher(|_key: &TxoRef, value: &Utxo| -> u32 {
                    // Get byte length of serialized UTxO
                    value.encode().len() as u32 + 36
                })
                .max_capacity(max_size_bytes)
                .build(),
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
}
