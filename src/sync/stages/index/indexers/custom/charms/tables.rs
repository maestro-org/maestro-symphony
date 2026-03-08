use maestro_symphony_macros::{Decode, Encode};

use crate::{
    define_indexer_table,
    storage::table::IndexerTable,
    sync::stages::index::indexers::{
        custom::TransactionIndexer,
        types::{ScriptPubKey, TxoRef},
    },
};

// ---

#[repr(u8)]
pub enum CharmsTables {
    CharmsUtxosByScript = 0,
    CharmsUtxosByApp = 1,
}

// ---

// Table to map script pubkey to references of controlled utxos which contain charms
define_indexer_table! {
    name: CharmsUtxosByScriptKV,
    key_type: CharmsUtxosByScriptKey,
    value_type: (),
    indexer: TransactionIndexer::Charms,
    table: CharmsTables::CharmsUtxosByScript
}

// Table to map charm app to references of utxos which contain that charm
define_indexer_table! {
    name: CharmsUtxosByAppKV,
    key_type: CharmsUtxosByAppKey,
    value_type: (),
    indexer: TransactionIndexer::Charms,
    table: CharmsTables::CharmsUtxosByApp
}

// ---

#[derive(Encode, Decode, Debug)]
pub struct CharmsUtxosByScriptKey {
    pub script: ScriptPubKey,
    pub produced_height: u64,
    pub txo_ref: TxoRef,
}

#[derive(Encode, Decode, Debug)]
pub struct CharmsUtxosByAppKey {
    /// Canonical charm app string as bytes (e.g. "t/identity/vk")
    pub app: Vec<u8>,
    pub produced_height: u64,
    pub txo_ref: TxoRef,
}

// ---

/// Charms attached to a UTXO, stored in Utxo.extended[TransactionIndexer::Charms].
/// Each entry: (app canonical string as bytes, Data CBOR bytes)
pub type UtxoCharms = Vec<(Vec<u8>, Vec<u8>)>;
