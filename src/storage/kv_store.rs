use std::{collections::HashMap, ops::Range, path::PathBuf, sync::Arc, u64};

use bitcoin::hashes::Hash;
use itertools::Itertools;
use maestro_symphony_macros::{Decode, Encode};
use rocksdb::{
    ColumnFamily, ColumnFamilyDescriptor, DB, Options, ReadOptions, Snapshot, WriteBatch,
};
use tracing::{debug, info};

use crate::{
    error::Error,
    sync::stages::{
        Point,
        index::indexers::core::rollback_buffer::{RollbackBufferKV, RollbackKey},
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

pub struct Task<'a> {
    pub reader: Snapshot<'a>, // TODO private
    cf_handle: &'a ColumnFamily,
    read_ts: Option<Timestamp>, // TODO
    // when we write keys, we do not write to storage, we manipulate here until we flush via write batch
    // when we read, we first check for the key here and if we dont find it we use the snapshot
    write_buffer: HashMap<RawKey, StorageAction>,
    // when we are maintaining a rollback buffer, we need to store original KVs for any modified keys
    original_kvs: Option<HashMap<RawKey, PreviousValue>>,
}

impl<'a> Task<'a> {
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
        read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts()); // TODO

        // Else read from the database
        self.reader
            .get_cf_opt(self.cf_handle, encoded_key, read_opts)?
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
        read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts()); // TODO

        let fetched = self.reader.multi_get_cf_opt(
            to_fetch.iter().map(|(_, enc_k)| (self.cf_handle, enc_k)),
            read_opts,
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
                read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts()); // TODO

                let original_value = self
                    .reader
                    .get_cf_opt(self.cf_handle, raw_key.clone(), read_opts)
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
        }
    }
}

pub struct FinalizedTask {
    pub write_buffer: HashMap<RawKey, StorageAction>,
    pub original_kvs: Option<HashMap<RawKey, PreviousValue>>,
}

#[derive(Clone)]
pub struct StorageHandler {
    pub db: Arc<DB>,
    pub previous_timestamp: Option<Timestamp>,
    // utxo cache TODO
}

impl StorageHandler {
    pub fn open(path: PathBuf) -> Self {
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

        let db = DB::open_cf_descriptors(&db_opts, path, cfs).unwrap();

        Self {
            db: Arc::new(db),
            previous_timestamp: None, // TODO detect from DB?
        }
    }

    pub fn cf_handle(&self) -> &ColumnFamily {
        self.db.cf_handle(SYMPHONY_CF_NAME).expect("cf missing")
    }

    pub fn begin_task(&mut self, mutable: bool) -> Task {
        let cf = self.cf_handle();

        Task {
            reader: self.db.snapshot(),
            read_ts: self.previous_timestamp.clone(),
            cf_handle: cf,
            write_buffer: HashMap::new(),
            original_kvs: mutable.then_some(HashMap::new()),
        }
    }

    /// Finish the task, by flushing all the pending writes to storage, along with the original KVs
    /// into the rollback buffer
    pub fn apply_task(
        &mut self,
        task: FinalizedTask,
        point: &Point,
        max_rollback: usize,
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
                let rollback_key = RollbackBufferKV::encode_key(&RollbackKey {
                    height: point.height,
                    hash: point.hash.to_byte_array(),
                    key,
                });

                wb.put_cf_with_ts(cf, rollback_key, ts, original.encode())
            }

            // remove old entries from persistent rollback buffer (maintain at most MAX_ROLLBACK
            // entries)
            let remove_before = point.height.saturating_sub(max_rollback as u64);
            let rbbuf_gc_range = RollbackBufferKV::encode_range(None::<&()>, Some(&remove_before));

            wb.delete_range_cf(cf, rbbuf_gc_range.start, rbbuf_gc_range.end);
        };

        // TODO: hide within write batch wrapper? or use tx?
        wb.update_timestamps_with_size(ts, U64Timestamp::SIZE)?;

        self.db.write(wb)?;

        // allow data for previous blocks to be GC'd
        self.db.increase_full_history_ts_low(cf, ts)?;

        self.previous_timestamp = Some(commit_ts);

        Ok(())
    }

    pub fn iter_kvs<'a, T: Table>(
        &self,
        snapshot: &'a rocksdb::SnapshotWithThreadMode<'a, DB>,
        range: Range<Vec<u8>>,
        ts: Timestamp,
        reverse: bool,
    ) -> TableIterator<'a, T> {
        let mut read_opts = ReadOptions::default();
        read_opts.set_timestamp(ts.as_rocksdb_ts());
        read_opts.set_iterate_range(range);

        let mode = if reverse {
            rocksdb::IteratorMode::End
        } else {
            rocksdb::IteratorMode::Start
        };

        let iter = snapshot.iterator_cf_opt(&self.cf_handle(), read_opts, mode);

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
