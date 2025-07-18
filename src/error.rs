use thiserror::Error;

use crate::{
    storage::encdec::DecodingError,
    sync::stages::{Point, index::indexers::types::TxoRef, pull::peer::P2PError},
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("decoding error: {0}")]
    Decoding(#[from] DecodingError),

    #[error("rocksdb error: {0}")]
    Rocks(#[from] rocksdb::Error),

    #[error("rollback point not within range: {0:?}")]
    Rollback(Point),

    #[error("{0}")]
    Custom(String),

    #[error("{0}")]
    P2P(#[from] P2PError),

    #[error("{0:?}")]
    MissingUtxo(TxoRef),

    #[error("invalid merge operator combination")]
    MergeOperator,
}

impl Error {
    pub fn config(error: Box<dyn std::error::Error>) -> Error {
        Error::Config(format!("{error}"))
    }

    pub fn custom(error: Box<dyn std::error::Error>) -> Error {
        Error::Custom(format!("{error}"))
    }

    pub fn missing_utxo(txo_ref: TxoRef) -> Error {
        Error::MissingUtxo(txo_ref)
    }
}

impl From<Box<dyn std::error::Error>> for Error {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        Error::custom(err)
    }
}
