use maestro_symphony_macros::{Decode, Encode};
use ordinals::RuneId;

use crate::{
    define_indexer_table,
    storage::{
        encdec::{Decode, Encode},
        table::IndexerTable,
    },
    sync::stages::index::indexers::{
        custom::TransactionIndexer,
        types::{ScriptPubKey, TxoRef},
    },
};

// ---

#[repr(u8)]
pub enum RunesTables {
    RuneInfoById = 0,
    RuneIdByName = 1,
    RuneMintsById = 2,
    RuneUtxosByScript = 3,
    RuneOpsByTx = 4,
}

// ---

// Table to map rune ID to rune terms
define_indexer_table! {
    name: RuneInfoByIdKV,
    key_type: RuneId,
    value_type: RuneInfo,
    indexer: TransactionIndexer::Runes,
    table: RunesTables::RuneInfoById
}

// Table to map rune name to rune ID
define_indexer_table! {
    name: RuneIdByNameKV,
    key_type: u128,
    value_type: RuneId,
    indexer: TransactionIndexer::Runes,
    table: RunesTables::RuneIdByName
}

// Table to track total number of mints for each rune ID
define_indexer_table! {
    name: RuneMintsByIdKV,
    key_type: RuneId,
    value_type: u128,
    indexer: TransactionIndexer::Runes,
    table: RunesTables::RuneMintsById
}

// Table to map script pubkey to references of controlled utxos which containing runes
define_indexer_table! {
    name: RuneUtxosByScriptKV,
    key_type: RuneUtxosByScriptKey,
    value_type: (), // fetch utxo from UtxoByTxoRef using ref from key
    indexer: TransactionIndexer::Runes,
    table: RunesTables::RuneUtxosByScript
}

// Table to log rune operations inside each transaction (optional)
define_indexer_table! {
    name: RuneOpsByTxKV,
    key_type: RuneOpsByTxKey,
    value_type: RuneBalanceChange,
    indexer: TransactionIndexer::Runes,
    table: RunesTables::RuneOpsByTx
}

// ---

#[derive(Encode, Decode, Debug)]
pub struct RuneUtxosByScriptKey {
    pub script: ScriptPubKey,
    pub produced_height: u64,
    pub txo_ref: TxoRef,
}

#[derive(Encode, Decode, Debug)]
pub struct RuneTerms {
    pub amount: Option<u128>,
    pub cap: Option<u128>,
    pub start_height: Option<u64>,
    pub end_height: Option<u64>,
}

#[derive(Encode, Decode, Debug)]
pub struct RuneInfo {
    pub name: u128,
    pub terms: Option<RuneTerms>,
    pub symbol: Option<u32>,
    pub divisibility: u8,
    pub etching_height: u64,
    pub etching_tx: [u8; 32],
    pub premine: u128,
    pub spacers: u32,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct RuneOpsByTxKey {
    /// Transaction hash
    pub tx_hash: [u8; 32],
    /// monotonically-increasing sequence number inside the tx to keep keys unique and ordered
    pub seq: u16,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct RuneBalanceChange {
    pub rune_id: RuneId,
    /// Script of the address.
    pub script: ScriptPubKey,
    /// Total amount of the rune spent by this address in the transaction.
    pub sent: u128,
    /// Total amount of the rune received by this address in the transaction (including mints).
    pub received: u128,
}

// ---

impl Encode for RuneId {
    fn encode(&self) -> Vec<u8> {
        (self.block, self.tx).encode()
    }
}

impl Decode for RuneId {
    fn decode(bytes: &[u8]) -> crate::DecodingResult<Self> {
        let ((block, tx), bytes) = <_>::decode(bytes)?;

        Ok((RuneId { block, tx }, bytes))
    }
}

pub type UtxoRunes = Vec<(RuneId, u128)>;
