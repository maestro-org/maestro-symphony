use std::str::FromStr;

use crate::storage::encdec::Decode;
use bitcoin::hashes::Hash;
use bitcoin::{Network, Txid};
use clap::{Parser, Subcommand};
use rocksdb::ReadOptions;
use serde::Deserialize;
use storage::{kv_store::StorageHandler, table::Table, timestamp::Timestamp};
use sync::stages::index::indexers::{
    core::utxo_by_txo_ref::{TxoRef, UtxoByTxoRefKV},
    custom::{
        TransactionIndexer,
        runes::tables::{RuneUtxosByScriptKV, RuneUtxosByScriptKey, UtxoRunes},
    },
};
use tracing::info;

pub use storage::encdec::{DecodingError, DecodingResult};

mod error;
pub mod storage;
pub mod sync;

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::init();

    let args = Cli::parse();

    info!("using db path: './tmp/symphony'");

    let db = StorageHandler::open("./tmp/symphony".into()); // TODO

    let cf = db.cf_handle();

    db.db.compact_range_cf(cf, None::<&[u8]>, None::<&[u8]>);
    db.db.flush().unwrap();

    match args.command {
        Command::Run(_) => {
            let config = Config::new(&args.config).unwrap(); // TODO: error handle

            info!("running symphony with config: {config:?}");

            sync::pipeline::pipeline(config.sync, db).unwrap().block()
        }
        Command::Query(query_args) => {
            // temporary query logic for testing

            let address = bitcoin::Address::from_str(&query_args.address).unwrap();
            let script = address
                .require_network(Network::Testnet4)
                .unwrap()
                .script_pubkey();

            // make key range to return all rune utxos for script
            let range_start = <RuneUtxosByScriptKV>::encode_key(&RuneUtxosByScriptKey {
                script: script.to_bytes(),
                produced_height: 0,
                txo_ref: TxoRef {
                    tx_hash: [0; 32],
                    txo_index: 0,
                },
            });

            let range_end = <RuneUtxosByScriptKV>::encode_key(&RuneUtxosByScriptKey {
                script: script.to_bytes(),
                produced_height: u64::MAX,
                txo_ref: TxoRef {
                    tx_hash: [0; 32],
                    txo_index: 0,
                },
            });

            println!(
                "utxos containing runes controlled by {} (divisibility ignored):",
                query_args.address
            );

            let mut read_opts = ReadOptions::default();
            read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts());

            let snapshot = db.db.snapshot();
            let iter = snapshot
                .iterator_cf_opt(
                    &cf,
                    read_opts,
                    rocksdb::IteratorMode::From(&range_start, rocksdb::Direction::Forward),
                )
                .filter(|x| x.as_ref().unwrap().0.as_ref() < range_end.as_slice());

            for kv in iter {
                let (raw_k, _) = kv.unwrap();

                let key = <RuneUtxosByScriptKV as Table>::Key::decode_all(&raw_k[4..]).unwrap(); // TODO: prefixed key decode

                let mut read_opts = ReadOptions::default();
                read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts());

                let res = snapshot
                    .get_cf_opt(&cf, UtxoByTxoRefKV::encode_key(&key.txo_ref), read_opts)
                    .unwrap()
                    .unwrap();

                let utxo_val = <UtxoByTxoRefKV as Table>::Value::decode_all(&res).unwrap();
                let utxo_runes_raw = utxo_val.extended.get(&TransactionIndexer::Runes).unwrap();

                let utxo_runes = UtxoRunes::decode_all(&utxo_runes_raw).unwrap();

                println!(
                    ">> {}#{} -> {} sats + {utxo_runes:?} ",
                    Txid::from_byte_array(key.txo_ref.tx_hash).to_string(),
                    key.txo_ref.txo_index,
                    utxo_val.satoshis
                )
            }

            // TODO
            // let key = hex::decode(&query_args.hex_key).unwrap();

            // let mut read_opts = ReadOptions::default();
            // read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts()); // TODO

            // let task = db.begin_task(false);

            // let rune_id = RuneId {
            //     block: 30562,
            //     tx: 50,
            // };

            // let res = task.get::<RuneInfoByIdKV>(&rune_id).unwrap();

            // println!("{res:?}")
        }
    }

    Ok(())
}

#[derive(Debug, Subcommand)]
enum Command {
    // Sync(sync::Args),
    // Serve(serve::Args),
    Run(Args),        // TODO
    Query(QueryArgs), // TODO
}

#[derive(Debug, clap::Args)]
pub struct Args {}

#[derive(Debug, clap::Args)]
pub struct QueryArgs {
    address: String,
}

#[derive(Debug, Parser)]
#[clap(name = "maestro-symphony")]
#[clap(bin_name = "maestro-symphony")]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    config: Option<std::path::PathBuf>,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub sync: sync::Config,
}

impl Config {
    pub fn new(config_path: &Option<std::path::PathBuf>) -> Result<Self, config::ConfigError> {
        let mut s = config::Config::builder();

        s = s.add_source(config::File::with_name("symphony.toml").required(false));

        if let Some(explicit) = config_path.as_ref().and_then(|x| x.to_str()) {
            s = s.add_source(config::File::with_name(explicit).required(true));
        }

        s = s.add_source(config::Environment::with_prefix("SYMPHONY").separator("_"));

        s.build()?.try_deserialize()
    }
}
