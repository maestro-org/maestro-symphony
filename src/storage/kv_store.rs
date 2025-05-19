use std::{collections::HashMap, path::PathBuf};

use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, DB, Options, Snapshot, WriteBatch};

use crate::error::Error;

use super::{
    encdec::{Decode, Encode},
    table::{IndexerTable, Table},
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
    pub fn maybe_store_original_kv(&mut self, raw_key: &RawKey) -> Result<(), Error> {
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

    fn next_ts(&self) -> Timestamp {
        Timestamp::new(self.previous_timestamp)
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
    pub fn finish_task(&mut self, task: Task) -> Result<(), Error> {
        let timestamp = Timestamp::new(self.previous_timestamp);

        // - peform the write buffer actions, using Timestamp

        let mut wb = WriteBatch::new();

        let ts = timestamp.as_rocksdb_ts();

        for (key, action) in task.write_buffer {
            match action {
                StorageAction::Set(value) => wb.put_cf_with_ts(task.cf_handle, key, ts, value),
                StorageAction::Delete => wb.delete_cf_with_ts(task.cf_handle, key, ts),
            }
        }

        // - insert the undo actions into the rollback buffer if mutable

        unimplemented!();

        // - insert a timestamp entry

        unimplemented!();

        // - finish

        self.task_live = false;
        self.previous_timestamp = timestamp.into();
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

// use should be able to define a db
// within a reducer, the user needs to be able to get, set, delete KVs in a specific table in a specific
