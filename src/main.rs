use std::str::FromStr;

use crate::storage::encdec::Decode;
use bitcoin::hashes::Hash;
use bitcoin::{Network, Txid};
use clap::{Parser, Subcommand};
use ordinals::RuneId;
use rocksdb::{IteratorMode, ReadOptions};
use serde::Deserialize;
use storage::table::Table;
use storage::{kv_store::StorageHandler, timestamp::Timestamp};
use sync::stages::index::indexers::custom::runes::tables::RuneInfoByIdKV;
use sync::stages::index::indexers::{
    core::utxo_by_txo_ref::UtxoByTxoRefKV,
    custom::{
        TransactionIndexer,
        runes::tables::{RuneUtxosByScriptKV, UtxoRunes},
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

    let mut db = StorageHandler::open("./tmp/symphony".into()); // TODO

    let cf = db.cf_handle();

    // info!("compacting/flushing db...");
    // db.db.compact_range_cf(cf, None::<&[u8]>, None::<&[u8]>);
    // db.db.flush().unwrap();

    match args.command {
        Command::Run(_) => {
            let config = Config::new(&args.config).unwrap(); // TODO: error handle

            info!("running symphony with config: {config:?}");

            sync::pipeline::pipeline(config.sync, db).unwrap().block()
        }
        Command::Query(query_args) => {
            info!("querying data...");
            // temporary query logic for testing

            if query_args.string == String::from("dump") {
                let snapshot = db.db.snapshot();

                let mut read_opts = ReadOptions::default();
                read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts());

                for x in snapshot.iterator_cf_opt(&cf, read_opts, IteratorMode::Start) {
                    let x = x.unwrap();

                    println!("{} -> {}", hex::encode(&x.0), hex::encode(&x.1));
                }
            } else if query_args.string.contains(':') {
                // rune ID query
                let mut parts = query_args.string.split(":");
                let block = parts.next().unwrap().parse().unwrap();
                let tx = parts.next().unwrap().parse().unwrap();

                let task = db.begin_task(false);

                let rune_id = RuneId { block, tx };

                let res = task.get::<RuneInfoByIdKV>(&rune_id).unwrap();

                if let Some(info) = res {
                    println!("rune info for {block}:{tx}:");
                    println!("{info:?}")
                } else {
                    println!("rune not found")
                }
            } else {
                let address = bitcoin::Address::from_str(&query_args.string).unwrap();
                let script = address
                    .require_network(Network::Testnet4)
                    .unwrap()
                    .script_pubkey();

                let range = <RuneUtxosByScriptKV>::encode_range(
                    Some(&(script.to_bytes(), u64::MIN)),
                    Some(&(script.to_bytes(), u64::MAX)),
                );

                println!(
                    "utxos containing runes controlled by {} (divisibility ignored):",
                    query_args.string
                );

                let snapshot = db.db.snapshot();
                let iter = db.iter_kvs::<RuneUtxosByScriptKV>(
                    &snapshot,
                    range,
                    Timestamp::from_u64(u64::MAX),
                    false,
                );

                for kv in iter {
                    let (key, _) = kv.unwrap();

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
            }
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
    string: String,
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
