pub mod hash_by_height;
pub mod indexer_info;
pub mod rollback_buffer;
pub mod timestamps;
pub mod utxo_by_txo_ref;

#[repr(u8)]
pub enum CoreIndexer {
    IndexerInfo = b'I',
    UtxoByTxoRef = b'U',
    Timestamps = b'T',
    HashByHeight = b'H',
    RollbackBuffer = b'R',
}
