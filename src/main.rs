use std::fs;

use crate::error::Error;
use crate::serve::openapi::APIDoc;
use crate::serve::{DEFAULT_SERVE_ADDRESS, ServerConfig};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use storage::kv_store::StorageHandler;
use tracing::{info, warn};

pub use storage::encdec::{DecodingError, DecodingResult};
use utoipa::OpenApi;

mod error;
pub mod serve;
pub mod storage;
pub mod sync;

#[derive(Debug, Subcommand)]
enum Command {
    Sync(SyncArgs),
    Serve(ServeArgs),
    Run(RunArgs),
    Docs(DocArgs),
}

#[derive(Debug, clap::Args)]
pub struct SyncArgs {}

#[derive(Debug, clap::Args)]
pub struct ServeArgs {}

#[derive(Debug, clap::Args)]
pub struct RunArgs {}

#[derive(Debug, clap::Args)]
pub struct DocArgs {}

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

            sync::pipeline::pipeline(config.sync, db, None)
                .unwrap()
                .block()
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

            let sync_db = db.clone();

            // Create channels to stop and start the sync and serve tasks
            let (sync_ended_tx, mut sync_ended_rx) = tokio::sync::mpsc::channel(1);
            let (serve_ended_tx, serve_ended_rx) = tokio::sync::oneshot::channel();
            let (start_sync_tx, start_sync_rx) = tokio::sync::oneshot::channel();
            let (start_serve_tx, start_serve_rx) = tokio::sync::oneshot::channel();

            // Spawn the sync task
            let _sync_handle = tokio::spawn(async move {
                let _ = start_sync_rx.await;

                info!("starting sync side...");

                let _ = sync::pipeline::pipeline(
                    config.sync,
                    sync_db,
                    Some((serve_ended_rx, sync_ended_tx)),
                )
                .unwrap();
            });

            // Spawn the serve task
            let serve_handle = tokio::spawn(async move {
                let _ = start_serve_rx.await;

                info!("starting serve side...");

                let res = serve::run(db, &serve_address).await;
                warn!(
                    "serve task ended with result: {:?}, telling sync side to stop...",
                    res
                );

                let _ = serve_ended_tx.send(());
            });

            let _ = start_sync_tx.send(());
            let _ = start_serve_tx.send(());

            let _ = sync_ended_rx.recv().await;

            serve_handle.abort();

            info!("symphony stopping...");
        }
        Command::Docs(_) => {
            if let Err(e) = fs::write(
                "docs/openapi.json",
                APIDoc::openapi().to_pretty_json().unwrap(),
            ) {
                warn!("unable to write swagger to docs/openapi.json: {e:?}")
            } else {
                info!("wrote docs/openapi.json");
            }
        }
    }

    Ok(())
}
