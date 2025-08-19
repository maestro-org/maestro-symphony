use sysinfo::System;
use tracing::info;

pub mod context;
pub mod stage;

fn get_default_cache_size() -> u64 {
    let system = System::new_all();

    let total_memory = system.total_memory();
    let default_budget = (total_memory as f64 * 0.1) as u64; // 10% of total memory

    info!(
        "No UTxO cache memory budget specified, using 10% of system memory: {:.2} GB ({} bytes) out of {:.2} GB total",
        default_budget as f64 / (1024.0 * 1024.0 * 1024.0),
        default_budget,
        total_memory as f64 / (1024.0 * 1024.0 * 1024.0)
    );

    default_budget
}
