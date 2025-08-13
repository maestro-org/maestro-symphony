pub mod context;
pub mod stage;

fn get_default_cache_size() -> u64 {
    // Default to 0.5GB (512MB)
    512 * 1024 * 1024
}
