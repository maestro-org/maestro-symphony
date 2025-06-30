use id::ProcessTransaction;
use maestro_symphony_macros::{Decode, Encode};
use runes::indexer::{RunesIndexer, RunesIndexerConfig};
use serde::Deserialize;
use tx_count_by_address::TxCountByAddressIndexer;

use crate::{
    error::Error, sync::stages::index::indexers::custom::utxos_by_address::UtxosByAddressIndexer,
};

pub mod id;
pub mod runes;
pub mod tx_count_by_address;
pub mod utxos_by_address;

/// Unique u8 for each transaction indexer, used in the key encodings. Do not modify, only add new
/// variants.
#[derive(Clone, Copy, Encode, Decode, PartialEq, Eq, std::hash::Hash, Debug)]
#[repr(u8)]
pub enum TransactionIndexer {
    TxCountByAddress = 0,
    Runes = 1,
    UtxosByAddress = 2,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type")]
pub enum TransactionIndexerFactory {
    TxCountByAddress,
    Runes(RunesIndexerConfig),
    UtxosByAddress,
}

impl TransactionIndexerFactory {
    pub fn create_indexer(self) -> Result<Box<dyn ProcessTransaction>, Error> {
        match self {
            Self::TxCountByAddress => Ok(Box::new(TxCountByAddressIndexer::new())),
            Self::Runes(c) => Ok(Box::new(RunesIndexer::new(c)?)),
            Self::UtxosByAddress => Ok(Box::new(UtxosByAddressIndexer::new())),
        }
    }
}

#[derive(Clone, Copy, Encode, Decode, PartialEq, Eq, std::hash::Hash)]
#[repr(u8)]
pub enum BlockIndexer {
    TimestampByHeight,
}
