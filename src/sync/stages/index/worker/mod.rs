pub mod context;
pub mod stage;

fn get_default_cache_size() -> u64 {
    512 * 1024 * 1024 // 512MB default
}
