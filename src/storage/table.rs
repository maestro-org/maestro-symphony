use super::encdec::{Decode, Encode, EncodeBuilder};

const CORE_INDEXER_TAG: u8 = b'C';
const CUSTOM_INDEXER_TAG: u8 = b'U';

/// Defines a CoreTable.
///
/// # Example
/// ```
/// define_core_table! {
///     name: UtxoByTxoRefKV,
///     key_type: TxoRef,
///     value_type: Utxo,
///     indexer: CoreIndexer::UtxoByTxoRef
/// }
/// ```
#[macro_export]
macro_rules! define_core_table {
    {
        name: $name:ident,
        key_type: $key_type:ty,
        value_type: $value_type:ty,
        indexer: $indexer_id:expr
    } => {
        pub struct $name;

        impl $crate::storage::table::TableBase for $name {
            type Key = $key_type;
            type Value = $value_type;
        }

        impl $crate::storage::table::CoreTable for $name {
            const INDEXER_ID: u8 = $indexer_id as u8;
        }
    };
}

/// Defines an IndexerTable.
///
/// # Example
/// ```
/// define_indexer_table! {
///     name: MyCustomTable,
///     key_type: MyKey,
///     value_type: MyValue,
///     indexer: CustomIndexer::MyIndex,
///     table: 1
/// }
/// ```
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

        impl $crate::storage::table::TableBase for $name {
            type Key = $key_type;
            type Value = $value_type;
        }

        impl $crate::storage::table::IndexerTable for $name {
            const INDEXER_ID: u8 = $indexer_id as u8;
            const TABLE_ID: u8 = $table_id;
        }
    };
}

/// Common trait with basic table requirements
pub trait TableBase {
    /// Key type for the table.
    type Key: Encode + Decode;

    /// Value type for the table.
    type Value: Encode + Decode;
}

/// A common trait for tables with a unique prefix and key-value types.
pub trait Table: TableBase {
    /// Encodes the full key by combining the indexer prefix, table prefix, and the encoded key.
    fn encode_key(key: &Self::Key) -> Vec<u8>;
}

/// Represents a table with a unique prefix and key-value types for core indexers.
pub trait CoreTable: TableBase {
    /// The core indexer this table belongs to.
    const INDEXER_ID: u8;
}

// Implementation for core tables
pub struct CoreTableImpl<T: CoreTable>(pub T);

impl<T: CoreTable> TableBase for CoreTableImpl<T> {
    type Key = T::Key;
    type Value = T::Value;
}

impl<T: CoreTable> Table for CoreTableImpl<T> {
    fn encode_key(key: &Self::Key) -> Vec<u8> {
        let mut enc = EncodeBuilder::new();
        enc = enc.append(&CORE_INDEXER_TAG);
        enc = enc.append(&T::INDEXER_ID);
        enc = enc.append(key);
        enc.build()
    }
}

/// Represents a table with a unique prefix and key-value types for custom indexers.
pub trait IndexerTable: TableBase {
    /// The customer indexer this table belongs to.
    const INDEXER_ID: u8;

    /// The unique prefix for this table, within the namespace of the indexer.
    const TABLE_ID: u8;
}

// Implementation for indexer tables
pub struct IndexerTableImpl<T: IndexerTable>(pub T);

impl<T: IndexerTable> TableBase for IndexerTableImpl<T> {
    type Key = T::Key;
    type Value = T::Value;
}

impl<T: IndexerTable> Table for IndexerTableImpl<T> {
    fn encode_key(key: &Self::Key) -> Vec<u8> {
        let mut enc = EncodeBuilder::new();
        enc = enc.append(&CUSTOM_INDEXER_TAG);
        enc = enc.append(&T::INDEXER_ID);
        enc = enc.append(&T::TABLE_ID);
        enc = enc.append(key);
        enc.build()
    }
}
