/// Represents a table that maps a transaction output reference (`TxoRef`) to an unspent transaction
/// output (`Utxo`).
///
/// This table is part of the core indexing system and is used to look up UTxOs by their transaction
/// hash and output index, so that when processing transactions we can find the UTxOs used as
/// inputs.
///
/// # Table Definition
/// - **Key**: [`TxoRef`] - A reference to a specific transaction output, consisting of a
/// transaction hash and an output index.
/// - **Value**: [`Utxo`] - The unspent transaction output, containing details such as the amount,
/// script, block height, and extended metadata.
/// - **Indexer**: [`CoreIndexer::UtxoByTxoRef`] - The indexer responsible for managing this table.
use std::collections::{HashMap, HashSet};

use bitcoin::hashes::Hash;

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
        mut cache: Option<UtxoCache>,
    ) -> Result<ResolvedUtxos, Error> {
        let tx_ids = txs
            .iter()
            .map(|x| x.tx_id.to_byte_array())
            .collect::<HashSet<_>>();

        let input_refs = txs
            .iter()
            .skip(1) // skip coinbase
            .flat_map(|x| x.tx.input.iter())
            .map(|x| TxoRef {
                tx_hash: x.previous_output.txid.to_byte_array(),
                txo_index: x.previous_output.vout,
            })
            .collect::<HashSet<_>>();

        let mut resolver = HashMap::with_capacity(input_refs.len());

        let mut fetch_from_db = Vec::with_capacity(input_refs.len());

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
                    None => panic!("missing non chained utxo {txo_ref:?}"), // TODO
                }
            }
        }

        Ok(ResolvedUtxos {
            resolver,
            chained_txos,
        })
    }
}

pub struct UtxoCache {
    map: HashMap<TxoRef, Utxo>,
    size: u64,
}

impl UtxoCache {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            size: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, txo_ref: &TxoRef) -> Option<&Utxo> {
        self.map.get(txo_ref)
    }

    pub fn remove(&mut self, txo_ref: &TxoRef) -> Option<Utxo> {
        if let Some(x) = self.map.remove(txo_ref) {
            self.size -= x.encode().len() as u64;
            Some(x)
        } else {
            None
        }
    }

    pub fn insert(&mut self, txo_ref: TxoRef, utxo: Utxo) -> Option<Utxo> {
        self.size += utxo.encode().len() as u64;
        self.map.insert(txo_ref, utxo)
    }

    pub fn contains_key(&self, txo_ref: &TxoRef) -> bool {
        self.map.contains_key(txo_ref)
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}
