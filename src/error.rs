use thiserror::Error;

use crate::DecodingError;

#[derive(Error, Debug)]
pub enum Error {
    #[error("decoding error: {0}")]
    Decoding(#[from] DecodingError),

    #[error("rocksdb error: {0}")]
    Rocks(#[from] rocksdb::Error),

    #[error("{0}")]
    Custom(String),
}

impl Error {
    pub fn custom(error: Box<dyn std::error::Error>) -> Error {
        Error::Custom(format!("{error}"))
    }
}

impl From<Box<dyn std::error::Error>> for Error {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        Error::custom(err)
    }
}
