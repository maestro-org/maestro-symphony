use bitcoin::{BlockHash, Transaction, Txid, block::Header};

pub mod health;
pub mod index;
pub mod pull;

pub type BlockHeight = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub height: BlockHeight,
    pub hash: BlockHash,
}

#[derive(Debug, Clone)]
pub struct TransactionWithId {
    tx_id: Txid,
    tx: Transaction,
}

pub type BlockTxs = Vec<TransactionWithId>;

#[derive(Debug, Clone)]
pub struct MempoolSnapshotInfo {
    fingerprint: [u8; 32],
    timestamp: u64,
    tip: Point,
}

#[derive(Debug, Clone)]
pub enum ChainEvent {
    RollForward(Point, Header, BlockTxs),
    RollBack(Point),
    MempoolBlocks(MempoolSnapshotInfo, Vec<BlockTxs>),
}
