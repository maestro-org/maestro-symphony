use bitcoin::{Block, consensus::Decodable, p2p::Magic};
use serde::Deserialize;
use stages::index::indexers::custom::TransactionIndexerFactory;

pub mod pipeline;
pub mod stages;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub node: NodeConfig,
    pub network: Network,
    /// Index estimated future blocks using transactions in mempool
    pub mempool: bool,
    /// Size in GB to use for UTxO cache (0 = disabled, default 0.5GB)
    pub utxo_cache_size: Option<f64>,

    /// Max number of blocks to pull from the node at once
    pub block_page_size: Option<usize>,

    /// Max in-flight messages between the pull and index stage
    pub stage_queue_size: Option<usize>,
    pub stage_timeout_secs: Option<u64>,

    pub indexers: IndexersConfig,

    pub max_rollback: Option<usize>,
    pub safe_mode: Option<bool>,
}

impl Config {
    pub fn utxo_cache_size_bytes(&self) -> Option<u64> {
        self.utxo_cache_size
            .map(|gb| (gb * 1024.0 * 1024.0 * 1024.0) as u64)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct NodeConfig {
    pub p2p_address: String,
    pub rpc_address: String,
    pub rpc_user: String,
    pub rpc_pass: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IndexersConfig {
    #[serde(default)]
    pub transaction_indexers: Vec<TransactionIndexerFactory>,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Testnet4,
    Regtest,
}

impl Network {
    pub fn genesis_block(&self) -> Block {
        match self {
            Self::Mainnet => bitcoin::constants::genesis_block(bitcoin::Network::Bitcoin),
            Self::Testnet4 => {
                let raw_block = hex::decode("0100000000000000000000000000000000000000000000000000000000000000000000004e7b2b9128fe0291db0693af2ae418b767e657cd407e80cb1434221eaea7a07a046f3566ffff001dbb0c78170101000000010000000000000000000000000000000000000000000000000000000000000000ffffffff5504ffff001d01044c4c30332f4d61792f323032342030303030303030303030303030303030303030303165626435386332343439373062336161396437383362623030313031316662653865613865393865303065ffffffff0100f2052a010000002321000000000000000000000000000000000000000000000000000000000000000000ac00000000").unwrap();
                Block::consensus_decode_from_finite_reader(&mut &raw_block[..]).unwrap()
            }
            Self::Regtest => bitcoin::constants::genesis_block(bitcoin::Network::Regtest),
        }
    }

    pub fn magic(&self) -> Magic {
        match self {
            Self::Mainnet => bitcoin::Network::Bitcoin.magic(),
            Self::Testnet4 => Magic::from_bytes([0x1c, 0x16, 0x3f, 0x28]),
            Self::Regtest => bitcoin::Network::Regtest.magic(),
        }
    }
}
