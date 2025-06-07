use std::{marker::PhantomData, ops::Range};

use rocksdb::DB;

use crate::error::Error;

use super::encdec::{Decode, Encode};

pub const CORE_INDEXER_TAG: u8 = b'C';
pub const CUSTOM_INDEXER_TAG: u8 = b'U';

/// Common trait with basic table requirements
pub trait Table {
    const PREFIX_LEN: usize;
    /// Key type for the table.
    type Key: Encode + Decode;
    /// Value type for the table.
    type Value: Encode + Decode;
    /// Encodes the full key including necessary prefixes
    fn encode_key(key: &Self::Key) -> Vec<u8>;
    /// Encodes a range with optional start and end bounds
    fn encode_range(start: Option<&impl Encode>, to: Option<&impl Encode>) -> Range<Vec<u8>>;
}

/// Represents a table with a unique prefix and key-value types for core indexers.
pub trait CoreTable: Table {
    const INDEXER_ID: u8;
}

/// Represents a table with a unique prefix and key-value types for custom indexers.
pub trait IndexerTable: Table {
    const INDEXER_ID: u16;
    const TABLE_ID: u8;
}

#[macro_export]
macro_rules! define_core_table {
    {
        name: $name:ident,
        key_type: $key_type:ty,
        value_type: $value_type:ty,
        indexer: $indexer_id:expr
    } => {
        pub struct $name;

        impl $crate::storage::table::CoreTable for $name {
            const INDEXER_ID: u8 = $indexer_id as u8;
        }

        impl $crate::storage::table::Table for $name {
            const PREFIX_LEN: usize = 2;
            type Key = $key_type;
            type Value = $value_type;

            fn encode_key(key: &Self::Key) -> Vec<u8> {
                let mut enc = $crate::storage::encdec::EncodeBuilder::new();
                enc = enc.append(&$crate::storage::table::CORE_INDEXER_TAG);
                enc = enc.append(&Self::INDEXER_ID);
                enc = enc.append(key);
                enc.build()
            }

            fn encode_range(start: Option<&impl $crate::storage::encdec::Encode>, to: Option<&impl $crate::storage::encdec::Encode>) -> std::ops::Range<Vec<u8>> {
                let mut prefix = $crate::storage::encdec::EncodeBuilder::new();
                prefix = prefix.append(&$crate::storage::table::CORE_INDEXER_TAG);
                prefix = prefix.append(&Self::INDEXER_ID);

                let prefix_range = $crate::storage::encdec::prefix_key_range(&prefix.clone().build());

                let mut start_enc = prefix.clone();
                let mut end_enc = prefix;

                if let Some(start) = start {
                    start_enc = start_enc.append(start);
                }

                let start_key = start_enc.build();

                let end_key = if let Some(to) = to {
                    end_enc = end_enc.append(to);
                    end_enc.build()
                } else {
                    prefix_range.end
                };

                start_key..end_key
            }
        }
    };
}

#[macro_export]
macro_rules! define_indexer_table {
    {
        name: $name:ident,
        key_type: $key_type:ty,
        value_type: $value_type:ty,
        indexer: $indexer_id:expr,
        table: $table_id:expr
    } => {
        pub struct $name;

        impl $crate::storage::table::IndexerTable for $name {
            const INDEXER_ID: u16 = $indexer_id as u16;
            const TABLE_ID: u8 = $table_id as u8;
        }

        impl $crate::storage::table::Table for $name {
            const PREFIX_LEN: usize = 4;
            type Key = $key_type;
            type Value = $value_type;

            fn encode_key(key: &Self::Key) -> Vec<u8> {
                let mut enc = $crate::storage::encdec::EncodeBuilder::new();
                enc = enc.append(&$crate::storage::table::CUSTOM_INDEXER_TAG);
                enc = enc.append(&Self::INDEXER_ID);
                enc = enc.append(&Self::TABLE_ID);
                enc = enc.append(key);
                enc.build()
            }

            fn encode_range(start: Option<&impl $crate::storage::encdec::Encode>, to: Option<&impl $crate::storage::encdec::Encode>) -> std::ops::Range<Vec<u8>> {
                let mut prefix = $crate::storage::encdec::EncodeBuilder::new();
                prefix = prefix.append(&$crate::storage::table::CUSTOM_INDEXER_TAG);
                prefix = prefix.append(&Self::INDEXER_ID);
                prefix = prefix.append(&Self::TABLE_ID);

                let prefix_range = $crate::storage::encdec::prefix_key_range(&prefix.clone().build());

                let mut start_enc = prefix.clone();
                let mut end_enc = prefix;

                if let Some(start) = start {
                    start_enc = start_enc.append(start);
                }

                let start_key = start_enc.build();

                let end_key = if let Some(to) = to {
                    end_enc = end_enc.append(to);
                    end_enc.build()
                } else {
                    prefix_range.end
                };

                start_key..end_key
            }
        }
    };
}

type RocksIterator<'a> = rocksdb::DBIteratorWithThreadMode<'a, DB>;

pub struct TableIterator<'a, T>(RocksIterator<'a>, PhantomData<T>);

impl<'a, T> TableIterator<'a, T> {
    pub fn new(inner: RocksIterator<'a>) -> Self {
        Self(inner, Default::default())
    }
}

impl<'a, T> Iterator for TableIterator<'a, T>
where
    T: Table,
{
    type Item = Result<(T::Key, T::Value), Error>;

    fn next(&mut self) -> Option<Result<(T::Key, T::Value), Error>> {
        match self.0.next() {
            Some(Ok((key, value))) => {
                let key_out = match T::Key::decode_all(&key[T::PREFIX_LEN..]) {
                    Ok(k) => k,
                    Err(e) => return Some(Err(e.into())),
                };

                let value_out = match T::Value::decode_all(&value[..]) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e.into())),
                };

                Some(Ok((key_out, value_out)))
            }
            Some(Err(err)) => Some(Err(err.into())),
            None => None,
        }
    }
}
