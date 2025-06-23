use std::{collections::HashMap, ops::Range, path::PathBuf, sync::Arc, u64};

use bitcoin::hashes::Hash;
use itertools::Itertools;
use maestro_symphony_macros::{Decode, Encode};
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, DB, Options, ReadOptions, WriteBatch};
use tracing::{debug, info};

use crate::{
    error::Error,
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
        if let Some(action) = self.write_buffer.get(&encoded_key) {
            debug!("fetching {} from writebuf", hex::encode(&encoded_key));

            match action {
                StorageAction::Set(value) => return Ok(Some(T::Value::decode_all(value)?)),
                StorageAction::Delete => return Ok(None),
            }
        }

        debug!("fetching {} from storage", hex::encode(&encoded_key));

        let mut read_opts = ReadOptions::default();
        read_opts.set_timestamp(self.read_ts.as_rocksdb_ts());

        // Else read from the database
        self.db
            .get_cf_opt(self.cf_handle, encoded_key, &read_opts)?
            .map(|x| T::Value::decode(&x).map(|y| y.0).map_err(|e| e.into()))
            .transpose()
    }

    pub fn multi_get<T>(&self, keys: Vec<T::Key>) -> Result<Vec<(T::Key, Option<T::Value>)>, Error>
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

        debug!("setting {}", hex::encode(&encoded_key));

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

        debug!("deleting {}", hex::encode(&encoded_key));

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
    pub fn open(path: PathBuf, read_only: bool) -> Self {
        info!("opening db...");
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let mut cf_opts = Options::default();
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
            .clone()
            .map(|x| Timestamp::after(x))
            .unwrap_or(Timestamp::new());

        let ts = commit_ts.as_rocksdb_ts();

        let cf = self.cf_handle();

        // apply storage actions
        for (key, action) in task.write_buffer {
            match action {
                StorageAction::Set(value) => wb.put_cf_with_ts(cf, key, ts, value),
                StorageAction::Delete => wb.delete_cf_with_ts(cf, key, ts),
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

                wb.put_cf_with_ts(cf, rollback_key, ts, original.encode())
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
        wb.put_cf_with_ts(
            cf,
            TimestampsKV::encode_key(&timestamps_key),
            ts,
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

        self.db.write(wb)?;

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
            T::encode_key(&key),
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

        let table_iter = TableIterator::<T>::new(iter);

        table_iter
    }
}

pub enum StorageAction {
    Set(RawValue),
    Delete,
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
