use bitcoin::hashes::Hash;
use indexmap::IndexMap;
use maestro_symphony_macros::{Decode, Encode};

use crate::sync::stages::index::indexers::custom::TransactionIndexer;

pub type ScriptPubKey = Vec<u8>;

#[derive(Encode, Decode, PartialEq, Hash, Eq, Clone, Copy, Debug)]
pub struct TxoRef {
    pub tx_hash: [u8; 32],
    pub txo_index: u32,
}

impl From<bitcoin::OutPoint> for TxoRef {
    fn from(outpoint: bitcoin::OutPoint) -> Self {
        Self {
            tx_hash: outpoint.txid.to_byte_array(),
            txo_index: outpoint.vout,
        }
    }
}

#[derive(Encode, Decode)]
pub struct Utxo {
    /// Amount of satoshis in the UTxO
    pub satoshis: u64,
    /// Script pubkey that controls the UTxO
    pub script: Vec<u8>,
    /// Height of block which produced the UTxO
    pub height: u64,
    /// Allow users to attach arbitrary data to UTxOs, so they don't need to spend timing resolving
    /// UTxOs again within their own indexers. Instead, they can inject their own data into our core
    /// indexing of UTxOs. For backwards compatability/extensibility we use a map of "data kind" to raw data,
    /// which the users can encode to/decode from as they choose. (TODO)
    pub extended: ExtendedUtxoData,
}

pub type ExtendedUtxoData = IndexMap<TransactionIndexer, Vec<u8>>;
