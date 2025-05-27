pub mod utxo_by_txo_ref;

#[repr(u8)]
pub enum CoreIndexer {
    UtxoByTxoRef,
}
