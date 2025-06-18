use thiserror::Error;

use crate::{
    storage::encdec::DecodingError,
    sync::stages::{Point, pull::peer::P2PError},
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
}

impl Error {
    pub fn config(error: Box<dyn std::error::Error>) -> Error {
        Error::Config(format!("{error}"))
    }

    pub fn custom(error: Box<dyn std::error::Error>) -> Error {
        Error::Custom(format!("{error}"))
    }
}

impl From<Box<dyn std::error::Error>> for Error {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        Error::custom(err)
    }
}
