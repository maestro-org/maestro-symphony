/// Result type for multi_get: a vector of (key, Option<value>) pairs.
pub type MultiGetResult<K, V> = Vec<(K, Option<V>)>;
use std::{collections::HashMap, ops::Range, path::PathBuf, sync::Arc};

use bitcoin::hashes::Hash;
use itertools::Itertools;
use maestro_symphony_macros::{Decode, Encode};
use rocksdb::{
    Cache, ColumnFamily, ColumnFamilyDescriptor, DB, Options, ReadOptions, SliceTransform,
    WriteBatch, WriteOptions,
};
use sysinfo::{Pid, System};
use tracing::{info, trace, warn};

use crate::{
    error::Error,
    storage::{
        encdec::encode::VarUIntEncoded,
        merge_operation::{
            MergeOperation, apply_merge_to_value, combine_merge_operations, create_merge_operator,
            create_partial_merge_operator,
        },
    },
    sync::stages::{
        Point,
        index::indexers::core::{
            rollback_buffer::{RollbackBufferKV, RollbackKey},
            timestamps::{PointKind, TimestampEntry, TimestampsKV},
        },
    },
};

use super::{
    encdec::{Decode, Encode},
    table::{Table, TableIterator},
    timestamp::{Timestamp, U64Comparator, U64Timestamp},
};

static SYMPHONY_CF_NAME: &str = "symphony";

pub type RawKey = Vec<u8>;
pub type RawValue = Vec<u8>;

pub struct IndexingTask<'a> {
    db: Arc<DB>,
    cf_handle: &'a ColumnFamily,
    // timestamp at which to read data at (we want data at commit ts of last rollforward/back)
    read_ts: Timestamp,
    // when we write keys, we do not write to storage, we manipulate here until we flush via write batch
    // when we read, we first check for the key here and if we dont find it we use the snapshot
    write_buffer: HashMap<RawKey, StorageAction>,
    // when we are maintaining a rollback buffer, we need to store original KVs for any modified keys
    original_kvs: Option<HashMap<RawKey, PreviousValue>>,
    // is this indexing task for mempool blocks
    mempool: bool,
}

impl<'a> IndexingTask<'a> {
    pub fn get<T>(&self, key: &T::Key) -> Result<Option<T::Value>, Error>
    where
        T: Table,
    {
        // Encode the key for the relevant table
        let encoded_key = T::encode_key(key);

        // Check the write buffer first
        let merge_op = if let Some(action) = self.write_buffer.get(&encoded_key) {
            trace!("fetching {} from writebuf", hex::encode(&encoded_key));

            match action {
                StorageAction::Set(value) => return Ok(Some(T::Value::decode_all(value)?)),
                StorageAction::Delete => return Ok(None),
                StorageAction::Merge(op) => Some(op),
            }
        } else {
            None
        };

        trace!(
            "fetching {} from storage (merge op: {merge_op:?})",
            hex::encode(&encoded_key)
        );

        self.get_inner::<T>(&encoded_key, merge_op)
    }

    /// Result type for multi_get: a vector of (key, Option<value>) pairs.
    pub fn multi_get<T>(&self, keys: Vec<T::Key>) -> Result<MultiGetResult<T::Key, T::Value>, Error>
    where
        T: Table,
    {
        // Encode the keys for the relevant table
        let keys = keys.into_iter().map(|key| {
            let encoded_key = T::encode_key(&key);
            (key, encoded_key)
        });

        let mut out = Vec::with_capacity(keys.len());

        let mut to_fetch = Vec::with_capacity(keys.len());

        for (key, encoded_key) in keys {
            // Check the write buffer first
            if let Some(action) = self.write_buffer.get(&encoded_key) {
                let value = match action {
                    StorageAction::Set(value) => Some(T::Value::decode_all(value)?),
                    StorageAction::Delete => None,
                    StorageAction::Merge(op) => self.get_inner::<T>(&encoded_key, Some(op))?,
                };

                out.push((key, value))
            } else {
                to_fetch.push((key, encoded_key))
            }
        }

        let mut read_opts = ReadOptions::default();
        read_opts.set_timestamp(self.read_ts.as_rocksdb_ts());

        let fetched = self.db.multi_get_cf_opt(
            to_fetch.iter().map(|(_, enc_k)| (self.cf_handle, enc_k)),
            &read_opts,
        );

        for ((key, _), value) in to_fetch.into_iter().zip_eq(fetched) {
            let value = match value? {
                Some(v) => Some(T::Value::decode_all(&v)?),
                None => None,
            };

            out.push((key, value));
        }

        Ok(out)
    }

    pub fn set<T>(&mut self, key: T::Key, value: T::Value) -> Result<(), Error>
    where
        T: Table,
    {
        // Encode the key and value
        let encoded_key = T::encode_key(&key);
        let encoded_value = value.encode();

        trace!("setting {}", hex::encode(&encoded_key));

        // Store original KV if mutable
        self.maybe_store_original_kv(&encoded_key)?;

        // Update the write buffer
        self.write_buffer
            .insert(encoded_key, StorageAction::Set(encoded_value));

        Ok(())
    }

    pub fn delete<T>(&mut self, key: T::Key) -> Result<(), Error>
    where
        T: Table,
    {
        // Encode the key and value
        let encoded_key = T::encode_key(&key);

        trace!("deleting {}", hex::encode(&encoded_key));

        // Store original KV if mutable
        self.maybe_store_original_kv(&encoded_key)?;

        // Update the write buffer
        self.write_buffer.insert(encoded_key, StorageAction::Delete);

        Ok(())
    }

    /// If we are mutable, this will store the original KV for any keys modified during the task,
    /// such that we can use them to later undo the effects of the current task on storage.
    fn maybe_store_original_kv(&mut self, raw_key: &RawKey) -> Result<(), Error> {
        if let Some(original_kvs) = self.original_kvs.as_mut() {
            // Only need to do once per key
            if !original_kvs.contains_key(raw_key) {
                let mut read_opts = ReadOptions::default();
                read_opts.set_timestamp(self.read_ts.as_rocksdb_ts()); // TODO

                let original_value = self
                    .db
                    .get_cf_opt(self.cf_handle, raw_key.clone(), &read_opts)
                    .map(|x| x.into())?;

                original_kvs.insert(raw_key.clone(), original_value);
            }
        }

        Ok(())
    }

    pub fn finalize(self) -> FinalizedTask {
        FinalizedTask {
            write_buffer: self.write_buffer,
            original_kvs: self.original_kvs,
            mempool: self.mempool,
        }
    }

    pub fn mempool(&self) -> bool {
        self.mempool
    }

    /// Generic merge operation helper
    fn merge_op<T>(&mut self, key: T::Key, op: MergeOperation) -> Result<(), Error>
    where
        T: Table,
    {
        let encoded_key = T::encode_key(&key);

        trace!(
            "applying {:?} operation on {}",
            op,
            hex::encode(&encoded_key)
        );

        // Store original KV if mutable
        self.maybe_store_original_kv(&encoded_key)?;

        // Check if there's already an operation for this key
        if let Some(existing_action) = self.write_buffer.get(&encoded_key) {
            match existing_action {
                // If there's already a Set operation, apply the merge to the existing value
                StorageAction::Set(existing_value) => {
                    // Apply the operation to the existing value
                    let result = apply_merge_to_value(&op, existing_value)?;

                    // Update the write buffer with a new Set operation
                    self.write_buffer
                        .insert(encoded_key, StorageAction::Set(result.encode()));

                    return Ok(());
                }
                // If there's already a Merge operation, try to combine them
                StorageAction::Merge(existing_op) => {
                    let merged_op = combine_merge_operations(existing_op, &op);

                    // Update the write buffer with the new Merge operation
                    self.write_buffer
                        .insert(encoded_key, StorageAction::Merge(merged_op));

                    return Ok(());
                }
                // For Delete operations, we can just replace with our merge
                StorageAction::Delete => (),
            }
        }

        // Update the write buffer with the new operation
        self.write_buffer
            .insert(encoded_key, StorageAction::Merge(op));

        Ok(())
    }

    /// Increment a numeric counter value without needing to read it first
    pub fn increment<T>(&mut self, key: T::Key, amount: T::Value) -> Result<(), Error>
    where
        T: Table,
        T::Value: VarUIntEncoded,
    {
        self.merge_op::<T>(key, MergeOperation::Increment(amount.as_u128()))
    }

    /// Decrement a numeric counter value without needing to read it first
    pub fn decrement<T>(&mut self, key: T::Key, amount: T::Value) -> Result<(), Error>
    where
        T: Table,
        T::Value: VarUIntEncoded,
    {
        self.merge_op::<T>(key, MergeOperation::Decrement(amount.as_u128()))
    }

    /// Helper method to get the value for a key from storage, which takes into account if there is
    /// a merge operation in the write buffer which needs to be applied to the fetched value
    fn get_inner<T>(
        &self,
        encoded_key: &[u8],
        merge_op: Option<&MergeOperation>,
    ) -> Result<Option<T::Value>, Error>
    where
        T: Table,
    {
        let mut read_opts = ReadOptions::default();
        read_opts.set_timestamp(self.read_ts.as_rocksdb_ts());

        // Get the original value from the database
        let base_value = self
            .db
            .get_cf_opt(self.cf_handle, encoded_key, &read_opts)?;

        // Apply any pending merge operations from the write buffer
        if let Some(op) = merge_op {
            match op {
                // Handle numeric operations (increment/decrement)
                MergeOperation::Increment(delta) | MergeOperation::Decrement(delta) => {
                    // Get the base value (or 0 if none)
                    // we are using increment, so it must be a varuint.
                    let base = match &base_value {
                        Some(bytes) => u128::decode_all(bytes)?,
                        None => 0,
                    };

                    // Apply the operation
                    let result = match op {
                        MergeOperation::Increment(_) => base.saturating_add(*delta),
                        MergeOperation::Decrement(_) => base.saturating_sub(*delta),
                    };

                    return Ok(Some(T::Value::decode_all(&result.encode())?));
                }
            }
        }

        // If we don't have any merge operations or don't know how to handle them,
        // just return the base value decoded as the expected type
        base_value
            .map(|bytes| T::Value::decode_all(&bytes).map_err(|e| e.into()))
            .transpose()
    }
}

pub struct FinalizedTask {
    pub write_buffer: HashMap<RawKey, StorageAction>,
    pub original_kvs: Option<HashMap<RawKey, PreviousValue>>,
    pub mempool: bool,
}

#[derive(Clone)]
pub struct StorageHandler {
    pub db: Arc<DB>,
    pub previous_timestamp: Option<Timestamp>, // TODO: move?
    read_only: bool,
}

impl StorageHandler {
    pub fn open(path: PathBuf, read_only: bool, memory_budget: u64) -> Self {
        info!("opening db...");
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        // Enable RocksDB statistics for monitoring
        db_opts.enable_statistics();
        db_opts.set_report_bg_io_stats(true);

        info!(
            "using rocksdb memory budget: {:.2} GB ({} bytes)",
            memory_budget as f64 / 1024.0 / 1024.0 / 1024.0,
            memory_budget
        );

        let block_cache_budget = (memory_budget as f64 * 0.75) as usize;
        let memtable_budget = (memory_budget as f64 * 0.25) as usize;

        let cache = Cache::new_lru_cache(block_cache_budget);

        let sys = System::new_all();
        let cpus = sys.cpus().len() as u32;
        let background_jobs = std::cmp::max(2, cpus);
        db_opts.set_max_background_jobs(background_jobs.try_into().unwrap());
        db_opts.set_max_subcompactions(cpus);

        let mut cf_opts = Options::default();

        let mut block_opts = rocksdb::BlockBasedOptions::default();
        block_opts.set_block_cache(&cache);
        cf_opts.set_block_based_table_factory(&block_opts);

        let per_memtable_cap = 512 * 1024 * 1024;
        cf_opts.set_write_buffer_size(std::cmp::min(memtable_budget / 2, per_memtable_cap));
        cf_opts.set_max_write_buffer_number(2);
        cf_opts.set_max_write_buffer_size_to_maintain(0);

        cf_opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(2));

        cf_opts.set_merge_operator(
            "extensible_merge",
            create_merge_operator(),
            create_partial_merge_operator(),
        );

        cf_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let cfs = vec![ColumnFamilyDescriptor::new(SYMPHONY_CF_NAME, cf_opts)];

        let db = if read_only {
            let mut secondary_path = path.clone();
            secondary_path.push("secondary");
            DB::open_cf_descriptors_as_secondary(&db_opts, path, secondary_path, cfs).unwrap()
        } else {
            DB::open_cf_descriptors(&db_opts, path, cfs).unwrap()
        };

        Self {
            db: Arc::new(db),
            previous_timestamp: None, // TODO detect from DB?
            read_only,
        }
    }

    /// Get RocksDB statistics as a formatted string
    pub fn get_statistics(&self) -> Option<String> {
        // RocksDB statistics are accumulated at the database level
        // We need to get them from the actual database instance
        // For now, we'll access via the DB property interface
        match self.db.property_value_cf(self.cf_handle(), "rocksdb.stats") {
            Ok(Some(stats)) => Some(stats),
            Ok(None) => {
                warn!("RocksDB statistics not available");
                None
            }
            Err(e) => {
                warn!("Failed to get RocksDB statistics: {}", e);
                None
            }
        }
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn cf_handle(&self) -> &ColumnFamily {
        self.db.cf_handle(SYMPHONY_CF_NAME).expect("cf missing")
    }

    pub fn begin_indexing_task(&mut self, mutable: bool) -> IndexingTask {
        let cf = self.cf_handle();

        IndexingTask {
            db: self.db.clone(),
            read_ts: self
                .previous_timestamp
                .unwrap_or(Timestamp::from_u64(u64::MAX)), // TODO
            cf_handle: cf,
            write_buffer: HashMap::new(),
            original_kvs: mutable.then_some(HashMap::new()),
            mempool: false,
        }
    }

    pub fn begin_mempool_indexing_task(&mut self) -> IndexingTask {
        let cf = self.cf_handle();

        IndexingTask {
            db: self.db.clone(),
            read_ts: self
                .previous_timestamp
                .unwrap_or(Timestamp::from_u64(u64::MAX)), // TODO
            cf_handle: cf,
            write_buffer: HashMap::new(),
            original_kvs: Some(HashMap::new()),
            mempool: true,
        }
    }

    /// Finish the task, by flushing all the pending writes to storage, along with the original KVs
    /// into the rollback buffer
    pub fn apply_indexing_task(
        &mut self,
        task: FinalizedTask,
        point: &Point,
        max_rollback: u64,
        mempool_timestamp: Option<u64>,
    ) -> Result<(), Error> {
        let mut wb = WriteBatch::new();

        let commit_ts = self
            .previous_timestamp
            .map(Timestamp::after)
            .unwrap_or_default();

        let ts = commit_ts.as_rocksdb_ts();

        let cf = self.cf_handle();

        // apply storage actions
        for (key, action) in task.write_buffer {
            match action {
                StorageAction::Set(value) => wb.put_cf(cf, key, value),
                StorageAction::Delete => wb.delete_cf(cf, key),
                StorageAction::Merge(op) => wb.merge_cf(cf, key, op.encode()),
            }
        }

        // write rollback buffer keys (TODO cleaner)
        if let Some(original_kvs) = task.original_kvs {
            for (key, original) in original_kvs {
                // use u64::MAX as height for mempool rbbuf entry
                let height = if task.mempool { u64::MAX } else { point.height };

                let rollback_key = RollbackBufferKV::encode_key(&RollbackKey {
                    height,
                    hash: point.hash.to_byte_array(),
                    key,
                });

                wb.put_cf(cf, rollback_key, original.encode())
            }

            if !task.mempool {
                // remove old entries from persistent rollback buffer (maintain at most MAX_ROLLBACK
                // entries)
                let remove_before = point.height.saturating_sub(max_rollback);
                let rbbuf_gc_range =
                    RollbackBufferKV::encode_range(None::<&()>, Some(&remove_before));

                wb.delete_range_cf(cf, rbbuf_gc_range.start, rbbuf_gc_range.end);
            }
        };

        let timestamps_key = if task.mempool {
            PointKind::Mempool
        } else {
            PointKind::Confirmed
        };

        // TODO: better to only update confirmed ts if point height is greater. if we do that, then
        // consider the timestamp low setting below will need to change
        wb.put_cf(
            cf,
            TimestampsKV::encode_key(&timestamps_key),
            TimestampEntry {
                tip_height: point.height,
                tip_hash: point.hash.to_byte_array(),
                rocks_timestamp: commit_ts.as_u64(),
                mempool_timestamp,
            }
            .encode(),
        );

        // TODO: hide within write batch wrapper? or use tx?
        wb.update_timestamps_with_size(ts, U64Timestamp::SIZE)?;

        let mut wopts = WriteOptions::default();
        wopts.disable_wal(true);

        self.db.write_opt(wb, &wopts)?;

        if !task.mempool {
            // allow data for blocks before this confirmed block to be GC'd
            self.db.increase_full_history_ts_low(cf, ts)?;
        }

        self.previous_timestamp = Some(commit_ts);

        Ok(())
    }

    pub fn reader(&self, timestamp: Timestamp) -> Reader {
        let mut read_opts = ReadOptions::default();
        read_opts.set_timestamp(timestamp.as_rocksdb_ts());

        Reader {
            db: self.db.clone(),
            timestamp,
            read_opts,
        }
    }

    pub fn try_refresh_read_only_data(&mut self) -> Result<(), Error> {
        if self.read_only {
            self.db.try_catch_up_with_primary()?
        }

        Ok(())
    }

    pub fn flush_and_compact(&mut self) -> Result<(), Error> {
        self.db.flush()?;
        self.db
            .compact_range_cf(self.cf_handle(), None::<Vec<u8>>, None::<Vec<u8>>);

        Ok(())
    }
}

// Pseudo-snapshot, which just reads data as of the specified timestamp
pub struct Reader {
    db: Arc<DB>,
    timestamp: Timestamp,
    read_opts: ReadOptions,
}

impl Reader {
    pub fn get<T>(&self, key: &T::Key) -> Result<Option<T::Value>, Error>
    where
        T: Table,
    {
        let res = self.db.get_cf_opt(
            self.db.cf_handle(SYMPHONY_CF_NAME).unwrap(),
            T::encode_key(key),
            &self.read_opts,
        )?;

        match res {
            Some(bytes) => Ok(Some(<T>::Value::decode_all(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn iter_kvs<T: Table>(&self, range: Range<Vec<u8>>, reverse: bool) -> TableIterator<'_, T> {
        let mut read_opts = ReadOptions::default();
        read_opts.set_timestamp(self.timestamp.as_rocksdb_ts());
        read_opts.set_iterate_range(range);

        let mode = if reverse {
            rocksdb::IteratorMode::End
        } else {
            rocksdb::IteratorMode::Start
        };

        let iter = self.db.iterator_cf_opt(
            self.db.cf_handle(SYMPHONY_CF_NAME).unwrap(),
            read_opts,
            mode,
        );

        TableIterator::<T>::new(iter)
    }
}

pub enum StorageAction {
    Set(RawValue),
    Delete,
    Merge(MergeOperation),
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum PreviousValue {
    Present(RawValue),
    NotPresent,
}

impl From<Option<RawValue>> for PreviousValue {
    fn from(option: Option<RawValue>) -> Self {
        match option {
            Some(value) => PreviousValue::Present(value),
            None => PreviousValue::NotPresent,
        }
    }
}

impl StorageHandler {
    pub fn print_perf_snapshot(&self) {
        let cf = self.cf_handle();

        let rocks_stats = self
            .db
            .property_value_cf(cf, "rocksdb.stats")
            .unwrap_or(Some("N/A".to_string()))
            .unwrap_or_default();
        let memtables = self
            .db
            .property_value_cf(cf, "rocksdb.cur-size-all-mem-tables")
            .unwrap_or(Some("0".to_string()))
            .unwrap_or_default();
        let block_cache = self
            .db
            .property_value("rocksdb.block-cache-usage")
            .unwrap_or(Some("0".to_string()))
            .unwrap_or_default();
        let block_cache_cf = self
            .db
            .property_value_cf(cf, "rocksdb.block-cache-usage")
            .unwrap_or(Some("0".to_string()))
            .unwrap_or_default();
        let pending_compaction = self
            .db
            .property_value("rocksdb.estimate-pending-compaction-bytes")
            .unwrap_or(Some("0".to_string()))
            .unwrap_or_default();
        let num_running_compactions = self
            .db
            .property_value("rocksdb.num-running-compactions")
            .unwrap_or(Some("0".to_string()))
            .unwrap_or_default();

        let sys = System::new_all();
        let pid = std::process::id();
        let process = sys.process(Pid::from_u32(pid)).unwrap();
        let app_mem_mb = process.memory() / 1024;

        let free_mem_mb = sys.free_memory() / 1024;
        let total_mem_mb = sys.total_memory() / 1024;

        println!("RocksDB Performance Stats");
        println!("App memory: {} KB", app_mem_mb);
        println!(
            "RocksDB memtables: {} MB",
            memtables.parse::<u64>().unwrap_or(0) / 1024 / 1024
        );
        println!(
            "RocksDB block cache: {} MB ({} cf)",
            block_cache.parse::<u64>().unwrap_or(0) / 1024 / 1024,
            block_cache_cf.parse::<u64>().unwrap_or(0) / 1024 / 1024
        );
        println!(
            "Pending compaction bytes: {} MB",
            pending_compaction.parse::<u64>().unwrap_or(0) / 1024 / 1024
        );
        println!("Running compactions: {}", num_running_compactions);
        println!(
            "System free memory: {} MB / {} MB",
            free_mem_mb, total_mem_mb
        );
        println!("RocksDB stats:\n{}", rocks_stats);
    }
}
