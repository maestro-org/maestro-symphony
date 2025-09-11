use sysinfo::System;
use tracing::info;

pub mod context;
pub mod stage;

fn get_default_cache_size() -> u64 {
    let mut system = System::new_all();

    system.refresh_memory();

    let total_memory = system
        .cgroup_limits()
        .map(|x| x.total_memory)
        .unwrap_or_else(|| system.total_memory());

    let default_budget = (total_memory as f64 * 0.25) as u64;

    info!(
        "No UTxO cache memory budget specified, using 25% of system memory: {:.2} GB ({} bytes) out of {:.2} GB total",
        default_budget as f64 / (1024.0 * 1024.0 * 1024.0),
        default_budget,
        total_memory as f64 / (1024.0 * 1024.0 * 1024.0)
    );

    default_budget
}
