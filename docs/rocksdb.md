# RocksDB

Symphony writes to RocksDB.

## Memory Usage

If no budget is specified in the config, Symphony by default will allocate 25% of the RAM to be shared by RocksDB block cache and memtables. Of that 25%, 25% or 1GB (which ever is smaller) will be shared by 2 memtables, and the remaining will be allocated to the block cache. If no budget is specified then indexes and filter blocks will not be stored in the block cache and therefore their memory usage will not be bounded and will grow as the database grows. In practice we see indexes and filter blocks use just less than 1.5GB RAM when for mainnet when fully synced (`table readers` in RocksDB stats log). If a budget is provided, we store indexes and filter blocks in the block cache via `rocksdb.cache_index_and_filter_blocks` which ensures their memory usage is bound by the block cache size, but this can affect performance.

It is recommended you use the default settings with the recommended deployment requirements. Setting a more conservative budget may harm performance. If you disabled the UTxO cache, which uses about ~40% RAM by default, then you could increase the memory allocated to RocksDB to use more of the available memory.