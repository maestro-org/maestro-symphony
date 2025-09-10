use std::time::Duration;
use tokio::time::interval;
use tracing::info;

use super::kv_store::StorageHandler;

/// Background task that logs RocksDB statistics every 5 minutes
pub async fn start_stats_logger(storage: StorageHandler) {
    let mut ticker = interval(Duration::from_secs(300));

    info!("Starting RocksDB statistics logger (interval: 5 minutes)");

    loop {
        ticker.tick().await;

        storage.print_perf_snapshot();
    }
}
