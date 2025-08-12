use bitcoin::{BlockHash, Transaction, Txid, block::Header};
use serde::Deserialize;

pub mod index;
pub mod pull;

pub type BlockHeight = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub height: BlockHeight,
    pub hash: BlockHash,
}

impl<'de> Deserialize<'de> for Point {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Only accept array format [height, hash]
        #[derive(Deserialize)]
        struct PointArray(BlockHeight, BlockHash);

        let point_array = PointArray::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("expected format: [height, \"hash\"]"))?;

        Ok(Point {
            height: point_array.0,
            hash: point_array.1,
        })
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
