use sysinfo::System;

pub mod context;
pub mod stage;

fn get_default_cache_size() -> u64 {
    let sys = System::new_all();
    sys.total_memory() / 6
}
