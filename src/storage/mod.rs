use serde::Deserialize;
use sysinfo::System;
use tracing::info;

pub mod encdec;
pub mod kv_store;
pub mod merge_operation;
pub mod stats_logger;
pub mod table;
pub mod timestamp;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    /// Total memory budget for RocksDB in GB (default 30% of available system memory)
    pub rocksdb_memory_budget: Option<f64>,
}

impl Config {
    pub fn rocksdb_memory_budget_bytes(&self) -> u64 {
        match self.rocksdb_memory_budget {
            Some(gb) => (gb * 1024.0 * 1024.0 * 1024.0) as u64,
            None => Self::default_rocksdb_memory_budget(),
        }
    }

    fn default_rocksdb_memory_budget() -> u64 {
        let mut system = System::new_all();

        system.refresh_memory();

        let total_memory = system
            .cgroup_limits()
            .map(|x| x.total_memory)
            .unwrap_or_else(|| system.total_memory());

        let default_budget = (total_memory as f64 * 0.4) as u64;

        info!(
            "No RocksDB memory budget specified, using 40% of system memory: {:.2} GB ({} bytes) out of {:.2} GB total",
            default_budget as f64 / (1024.0 * 1024.0 * 1024.0),
            default_budget,
            total_memory as f64 / (1024.0 * 1024.0 * 1024.0)
        );

        default_budget
    }
}
