use id::ProcessTransaction;
use maestro_symphony_macros::{Decode, Encode};
use serde::Deserialize;
use tx_count_by_address::{TxCountByAddressConfig, TxCountByAddressIndexer};

use crate::error::Error;

pub mod id;
mod runes;
mod tx_count_by_address;

/// Unique u8 for each transaction indexer, used in the key encodings. Do not modify, only add new
/// variants.
#[derive(Clone, Copy, Encode, Decode, PartialEq, Eq, std::hash::Hash)]
#[repr(u8)]
pub enum TransactionIndexer {
    TxCountByAddress = 0,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type")]
pub enum TransactionIndexerFactory {
    TxCountByAddress(TxCountByAddressConfig),
}

impl TransactionIndexerFactory {
    pub fn create_indexer(self) -> Result<Box<dyn ProcessTransaction>, Error> {
        match self {
            Self::TxCountByAddress(c) => Ok(Box::new(TxCountByAddressIndexer::new(c)?)),
        }
    }
}

#[derive(Clone, Copy, Encode, Decode, PartialEq, Eq, std::hash::Hash)]
#[repr(u8)]
pub enum BlockIndexer {
    TimestampByHeight,
}
