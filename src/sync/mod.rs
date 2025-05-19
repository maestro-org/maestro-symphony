use bitcoin::{Block, consensus::Decodable, p2p::Magic};
use serde::Deserialize;

mod pipeline;
mod stages;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub node_address: String,
    pub network: Network,

    pub block_page_size: Option<usize>,

    pub stage_queue_size: Option<usize>,
    pub stage_timeout_secs: Option<u64>,
    // utxo cache
    // rb buffer
    // mempool
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub enum Network {
    Mainnet,
    Testnet,
}

impl Network {
    // TODO
    pub fn genesis_block(&self) -> Block {
        match self {
            Self::Mainnet => bitcoin::constants::genesis_block(bitcoin::Network::Bitcoin),
            Self::Testnet => {
                let raw_block = hex::decode("0100000000000000000000000000000000000000000000000000000000000000000000004e7b2b9128fe0291db0693af2ae418b767e657cd407e80cb1434221eaea7a07a046f3566ffff001dbb0c78170101000000010000000000000000000000000000000000000000000000000000000000000000ffffffff5504ffff001d01044c4c30332f4d61792f323032342030303030303030303030303030303030303030303165626435386332343439373062336161396437383362623030313031316662653865613865393865303065ffffffff0100f2052a010000002321000000000000000000000000000000000000000000000000000000000000000000ac00000000").unwrap();
                Block::consensus_decode_from_finite_reader(&mut &raw_block[..]).unwrap()
            }
        }
    }

    pub fn magic(&self) -> Magic {
        match self {
            Self::Mainnet => bitcoin::Network::Bitcoin.magic(),
            Self::Testnet => Magic::from_bytes([0x1c, 0x16, 0x3f, 0x28]),
        }
    }
}
