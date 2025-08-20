#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

use std::fs;

use crate::error::Error;
use crate::serve::openapi::APIDoc;
use crate::serve::{DEFAULT_SERVE_ADDRESS, ServerConfig};
use crate::shutdown::ShutdownManager;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use storage::kv_store::StorageHandler;
use tracing::{info, warn};

pub use storage::encdec::{DecodingError, DecodingResult};
use utoipa::OpenApi;

mod error;
pub mod serve;
mod shutdown;
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
    pub storage: Option<storage::Config>,
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

    /// Get the RocksDB memory budget from storage config or return the default value
    pub fn rocksdb_memory_budget(&self) -> u64 {
        self.storage
            .as_ref()
            .map(|s| s.rocksdb_memory_budget_bytes())
            .unwrap_or_else(|| {
                storage::Config {
                    rocksdb_memory_budget: None,
                }
                .rocksdb_memory_budget_bytes()
            })
    }
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    // Initialize tracing with env filter to respect RUST_LOG
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stdout)
        .with_target(false)
        .with_ansi(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global default subscriber");

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

    // Setup shutdown handler
    let shutdown_manager = ShutdownManager::new();

    match args.command {
        Command::Sync(_) => {
            let db = StorageHandler::open(db_path.into(), false, config.rocksdb_memory_budget());

            info!(
                "running symphony in sync mode with config: {:?}",
                config.sync
            );

            let daemon = sync::pipeline::pipeline(config.sync, db, None).unwrap();

            // Since daemon.block() is not async, run it in a separate thread
            let (block_done_tx, block_done_rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                daemon.block();
                let _ = block_done_tx.send(());
            });

            // Wait for either shutdown or completion
            shutdown_manager
                .run_until_shutdown(async {
                    let _ = block_done_rx.await;
                    info!("Sync pipeline completed");
                })
                .await;
        }
        Command::Serve(_) => {
            let db = StorageHandler::open(db_path.into(), true, config.rocksdb_memory_budget());

            info!(
                "running symphony in serve mode with config: {:?}",
                config.server
            );

            // Run the server until shutdown
            if let Some(Err(e)) = shutdown_manager
                .run_until_shutdown(serve::run(db, &serve_address))
                .await
            {
                warn!("Serve mode ended with error: {e:?}");
            }
        }
        Command::Run(_) => {
            let db = StorageHandler::open(db_path.into(), false, config.rocksdb_memory_budget());

            info!("running symphony in sync+serve mode with config: {config:?}",);

            let sync_db = db.clone();

            // Create channels to stop and start the sync and serve tasks
            let (sync_ended_tx, mut sync_ended_rx) = tokio::sync::mpsc::channel(1);
            let (serve_ended_tx, serve_ended_rx) = tokio::sync::oneshot::channel();
            let (start_sync_tx, start_sync_rx) = tokio::sync::oneshot::channel();
            let (start_serve_tx, start_serve_rx) = tokio::sync::oneshot::channel();

            // Spawn the sync task
            tokio::spawn(async move {
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

            // Send start signals
            let _ = start_sync_tx.send(());
            let _ = start_serve_tx.send(());

            // Create a future that will wait for sync to end
            let sync_end_future = async {
                let _ = sync_ended_rx.recv().await;
                serve_handle.abort();
                info!("symphony stopping normally...");
            };

            // Run until either shutdown or sync ends
            shutdown_manager.run_until_shutdown(sync_end_future).await;
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

    info!("Normal shutdown complete");
    Ok(())
}
