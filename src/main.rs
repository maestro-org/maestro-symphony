use crate::serve::{DEFAULT_SERVE_ADDRESS, ServerConfig};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use storage::kv_store::StorageHandler;
use tracing::info;

pub use storage::encdec::{DecodingError, DecodingResult};

mod error;
pub mod serve;
pub mod storage;
pub mod sync;

#[derive(Debug, Subcommand)]
enum Command {
    Sync(SyncArgs),
    Serve(ServeArgs),
    Run(RunArgs),
}

#[derive(Debug, clap::Args)]
pub struct SyncArgs {}

#[derive(Debug, clap::Args)]
pub struct ServeArgs {}

#[derive(Debug, clap::Args)]
pub struct RunArgs {}

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
    pub db_path: Option<String>,
    pub sync: sync::Config,
    pub server: Option<ServerConfig>,
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

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::init();

    let args = Cli::parse();

    let config = Config::new(&args.config).unwrap();

    let db_path = config
        .db_path
        .clone()
        .unwrap_or_else(|| "./tmp/symphony".into());

    info!("using db path: '{}'", db_path);

    let serve_address = config
        .server
        .as_ref()
        .and_then(|s| s.address.clone())
        .unwrap_or_else(|| DEFAULT_SERVE_ADDRESS.to_string());

    match args.command {
        Command::Sync(_) => {
            let db = StorageHandler::open(db_path.into(), false);

            info!(
                "running symphony in sync mode with config: {:?}",
                config.sync
            );

            sync::pipeline::pipeline(config.sync, db).unwrap().block()
        }
        Command::Serve(_) => {
            let db = StorageHandler::open(db_path.into(), true);

            info!(
                "running symphony in serve mode with config: {:?}",
                config.server
            );

            serve::run(db, &serve_address).await.unwrap()
        }
        Command::Run(_) => {
            let db = StorageHandler::open(db_path.into(), false);

            info!(
                "running symphony in sync+serve mode with config: {:?}",
                config
            );

            // TODO: serve stage should stop if sync stage finishes
            let sync_db = db.clone();
            let sync_handle = tokio::spawn(async move {
                sync::pipeline::pipeline(config.sync, sync_db)
                    .unwrap()
                    .block()
            });

            let serve_result = serve::run(db, &serve_address).await;

            sync_handle.abort();

            info!("serve stage ended: {serve_result:?}")
        }
    }

    Ok(())
}
