use std::{collections::HashMap, path::PathBuf};

use itertools::Itertools;
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, DB, Options, Snapshot, WriteBatch};

use crate::error::Error;

use super::{
    encdec::{Decode, Encode},
    table::Table,
    timestamp::{Timestamp, U64Comparator, U64Timestamp},
};

static DEFAULT_COLUMN_FAMILY_NAME: &str = "cf";

pub type RawKey = Vec<u8>;
pub type RawValue = Vec<u8>;

pub struct Task<'a> {
    reader: Snapshot<'a>,
    cf_handle: &'a ColumnFamily,
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
            match action {
                StorageAction::Set(value) => return Ok(Some(T::Value::decode_all(value)?)),
                StorageAction::Delete => return Ok(None),
            }
        }

        // Else read from the database
        self.reader
            .get_cf(self.cf_handle, encoded_key)?
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

        let fetched = self
            .reader
            .multi_get_cf(to_fetch.iter().map(|(_, enc_k)| (self.cf_handle, enc_k)));

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
                let original_value = self
                    .reader
                    .get_cf(self.cf_handle, raw_key.clone())
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
    write_buffer: HashMap<RawKey, StorageAction>,
    original_kvs: Option<HashMap<RawKey, PreviousValue>>,
}

pub struct StorageHandler {
    pub db: DB,
    pub task_live: bool,
    pub previous_timestamp: Option<Timestamp>,
    // utxo cache TODO
    // rollback buffer TODO
}

impl StorageHandler {
    pub fn open(path: PathBuf) -> Self {
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

        let cfs = vec![ColumnFamilyDescriptor::new(
            DEFAULT_COLUMN_FAMILY_NAME,
            cf_opts,
        )];

        let db = DB::open_cf_descriptors(&db_opts, path, cfs).unwrap();

        Self {
            db,
            task_live: false,
            previous_timestamp: None, // TODO detect from DB
        }
    }

    pub fn begin_task(&mut self, mutable: bool) -> Task {
        // only one live task at a time
        assert!(self.task_live == false);

        self.task_live = true;
        let cf = self.db.cf_handle(DEFAULT_COLUMN_FAMILY_NAME).unwrap();

        Task {
            reader: self.db.snapshot(),
            cf_handle: cf,
            write_buffer: HashMap::new(),
            original_kvs: mutable.then_some(HashMap::new()),
        }
    }

    /// Finish the task, by flushing all the pending writes to storage, along with the original KVs
    /// into the rollback buffer
    pub fn apply_task(&mut self, task: FinalizedTask) -> Result<(), Error> {
        let mut wb = WriteBatch::new();

        let commit_ts = self
            .previous_timestamp
            .clone()
            .map(|x| Timestamp::after(x))
            .unwrap_or(Timestamp::new());

        let ts = commit_ts.as_rocksdb_ts();

        let cf = self.db.cf_handle(DEFAULT_COLUMN_FAMILY_NAME).unwrap();

        for (key, action) in task.write_buffer {
            match action {
                StorageAction::Set(value) => wb.put_cf_with_ts(cf, key, ts, value),
                StorageAction::Delete => wb.delete_cf_with_ts(cf, key, ts),
            }
        }

        // TODO: rollback buffer

        self.db.write(wb)?;

        self.previous_timestamp = Some(commit_ts);

        Ok(())
    }
}

pub enum StorageAction {
    Set(RawValue),
    Delete,
}

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
