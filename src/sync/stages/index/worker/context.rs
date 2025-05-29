use std::collections::{HashMap, HashSet};

use bitcoin::{BlockHash, Network, hashes::Hash};

use crate::{
    error::Error,
    storage::{encdec::Encode, kv_store::Task},
    sync::{
        self,
        stages::{
            BlockHeight, BlockTxs, Point, TransactionWithId,
            index::indexers::{
                core::utxo_by_txo_ref::{
                    ExtendedUtxoData, ResolvedUtxos, TxoRef, Utxo, UtxoByTxoRefKV,
                },
                custom::TransactionIndexer,
            },
        },
    },
};

// A context object that encapsulates all the data needed for indexing a transaction
pub struct IndexingContext {
    point: Point,
    resolver: HashMap<TxoRef, Utxo>,
    utxo_metadata: HashMap<TxoRef, ExtendedUtxoData>,
    chained_txos: HashSet<TxoRef>,
    network: Network,
}

// Public methods available to custom indexers
impl IndexingContext {
    pub fn block_height(&self) -> BlockHeight {
        self.point.height
    }

    pub fn block_hash(&self) -> BlockHash {
        self.point.hash
    }

    pub fn resolver(&self) -> &HashMap<TxoRef, Utxo> {
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
        task: &mut Task,
        txs: &BlockTxs,
        point: Point,
        network: sync::Network,
    ) -> Result<Self, Error> {
        let ResolvedUtxos {
            resolver,
            chained_txos,
        } = UtxoByTxoRefKV::resolve_inputs(&task, txs, None)?; // TODO cache

        let network = match network {
            sync::Network::Mainnet => Network::Bitcoin,
            sync::Network::Testnet => Network::Testnet4,
        };

        Ok(Self {
            point,
            resolver,
            utxo_metadata: Default::default(),
            chained_txos,
            network,
        })
    }

    pub(super) fn update_utxo_set(
        &mut self,
        task: &mut Task,
        tx: &TransactionWithId,
    ) -> Result<(), Error> {
        // remove consumed utxos from resolver and strorage
        for input in &tx.tx.input {
            let txo_ref = input.previous_output.into();
            self.resolver.remove(&txo_ref);
            task.delete::<UtxoByTxoRefKV>(txo_ref)?;
        }

        // for produced utxos, add those which are consumed later in the block to the resolver, and
        // the rest to storage
        for (output_index, output) in tx.tx.output.iter().enumerate() {
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
            } else {
                task.set::<UtxoByTxoRefKV>(txo_ref, utxo)?;
            }
        }

        Ok(())
    }
}
