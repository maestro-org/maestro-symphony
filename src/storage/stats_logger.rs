use std::time::Duration;
use tokio::time::interval;

use super::kv_store::StorageHandler;

pub async fn start_stats_logger(storage: StorageHandler) {
    let mut ticker = interval(Duration::from_secs(15 * 60));

    loop {
        ticker.tick().await;

        storage.print_perf_snapshot();
    }
}
