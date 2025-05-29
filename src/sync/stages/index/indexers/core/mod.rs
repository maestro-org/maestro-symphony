pub mod hash_by_height;
pub mod indexer_info;
pub mod point_by_timestamp;
pub mod utxo_by_txo_ref;

#[repr(u8)]
pub enum CoreIndexer {
    IndexerInfo = b'I',
    UtxoByTxoRef = b'U',
    PointByTimestamp = b'P',
    HashByHeight = b'H',
}
