use super::encdec::{Decode, Encode, EncodeBuilder};

pub const CORE_INDEXER_TAG: u8 = b'C';
pub const CUSTOM_INDEXER_TAG: u8 = b'U';

/// Common trait with basic table requirements
pub trait Table {
    /// Key type for the table.
    type Key: Encode + Decode;
    /// Value type for the table.
    type Value: Encode + Decode;
    /// Encodes the full key including necessary prefixes
    fn encode_key(key: &Self::Key) -> Vec<u8>;
}

/// Represents a table with a unique prefix and key-value types for core indexers.
pub trait CoreTable: Table {
    const INDEXER_ID: u8;

    fn encode_key(key: &Self::Key) -> Vec<u8> {
        let mut enc = EncodeBuilder::new();
        enc = enc.append(&CORE_INDEXER_TAG);
        enc = enc.append(&Self::INDEXER_ID);
        enc = enc.append(key);
        enc.build()
    }
}

/// Represents a table with a unique prefix and key-value types for custom indexers.
pub trait IndexerTable: Table {
    const INDEXER_ID: u16;
    const TABLE_ID: u8;

    fn encode_key(key: &Self::Key) -> Vec<u8> {
        let mut enc = EncodeBuilder::new();
        enc = enc.append(&CUSTOM_INDEXER_TAG);
        enc = enc.append(&Self::INDEXER_ID);
        enc = enc.append(&Self::TABLE_ID);
        enc = enc.append(key);
        enc.build()
    }
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
            type Key = $key_type;
            type Value = $value_type;

            fn encode_key(key: &Self::Key) -> Vec<u8> {
                let mut enc = $crate::storage::encdec::EncodeBuilder::new();
                enc = enc.append(&$crate::storage::table::CORE_INDEXER_TAG);
                enc = enc.append(&Self::INDEXER_ID);
                enc = enc.append(key);
                enc.build()
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
        }
    };
}
