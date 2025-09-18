use bitcoin::{BlockHash, Transaction, Txid, block::Header};

pub mod index;
pub mod pull;

pub type BlockHeight = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub height: BlockHeight,
    pub hash: BlockHash,
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.height, self.hash)
    }
}

#[derive(Debug, Clone)]
pub struct TransactionWithId {
    tx_id: Txid,
    tx: Transaction,
}

pub type BlockTxs = Vec<TransactionWithId>;

#[derive(Debug, Clone)]
pub struct MempoolSnapshotInfo {
    // fingerprint: [u8; 32],
    /// timestamp when snapshot created
    timestamp: u64,
    /// hash of chain tip block that mempool blocks chained on
    tip: BlockHash,
}

pub type TipEstimate = Point;

#[derive(Debug, Clone)]
pub enum ChainEvent {
    RollForward(Point, Header, BlockTxs, TipEstimate),
    RollBack(Point),
    MempoolBlocks(MempoolSnapshotInfo, Vec<BlockTxs>),
}
