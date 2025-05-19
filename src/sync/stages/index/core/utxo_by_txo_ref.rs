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
use indexmap::IndexMap;
use maestro_symphony_macros::{Decode, Encode};

use crate::{define_core_table, sync::stages::index::custom::CustomIndexer};

use super::CoreIndexer;

define_core_table! {
    name: UtxoByTxoRefKV,
    key_type: TxoRef,
    value_type: Utxo,
    indexer: CoreIndexer::UtxoByTxoRef
}

#[derive(Encode, Decode)]
pub struct TxoRef {
    tx_hash: [u8; 32],
    txo_index: u32,
}

#[derive(Encode, Decode)]
pub struct Utxo {
    /// Amount of satoshis in the UTxO
    satoshis: u64,
    /// Script pubkey that controls the UTxO
    script: Vec<u8>,
    /// Height of block which produced the UTxO
    height: u64,
    /// Allow users to attach arbitrary data to UTxOs, so they don't need to spend timing resolving
    /// UTxOs again within their own indexers. Instead, they can inject their own data into our core
    /// indexing of UTxOs. For backwards compatability/extensibility we use a map of "data kind" to raw data,
    /// which the users can encode to/decode from as the choose. (TODO)
    extended: IndexMap<CustomIndexer, Vec<u8>>,
}
