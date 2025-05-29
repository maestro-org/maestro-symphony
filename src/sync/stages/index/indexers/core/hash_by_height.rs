use crate::storage::timestamp::Timestamp;
use crate::storage::{encdec::Decode, table::Table};
use crate::sync::stages::BlockHash;
use bitcoin::hashes::Hash;
use rocksdb::{ColumnFamily, ReadOptions, Snapshot};

use crate::{define_core_table, error::Error, storage::table::CoreTable, sync::stages::Point};

use super::CoreIndexer;

define_core_table! {
    name: HashByHeightKV,
    key_type: u64,
    value_type: [u8; 32],
    indexer: CoreIndexer::HashByHeight
}

impl HashByHeightKV {
    pub fn intersect_options(
        snapshot: &Snapshot,
        cf: &ColumnFamily, // TODO: cleanup
        genesis_hash: BlockHash,
    ) -> Result<Vec<Point>, Error> {
        let mut out = vec![];

        // TODO: helpers
        let range_start = <Self as Table>::encode_key(&0);
        let range_end = <Self as Table>::encode_key(&u64::MAX);

        let mut read_opts = ReadOptions::default();
        read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts());

        // TODO: unwrap
        let mut iter = snapshot
            .iterator_cf_opt(
                &cf,
                read_opts,
                rocksdb::IteratorMode::From(&range_end, rocksdb::Direction::Reverse),
            )
            .filter(|x| x.as_ref().unwrap().0.as_ref() >= range_start.as_slice());

        let last_entry_height = match iter.next() {
            Some(res) => {
                let (raw_k, _) = res.map_err(Error::Rocks)?;

                let height = <Self as Table>::Key::decode_all(&raw_k[2..])?; // TODO: prefixed key decode

                Some(height)
            }
            None => None,
        };

        let mut indexes = vec![];

        let mut read_opts = ReadOptions::default();
        read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts());

        if let Some(tip_height) = last_entry_height {
            let mut step = 1;
            let mut index = tip_height;

            loop {
                indexes.push(index);

                if indexes.len() >= 10 {
                    step *= 2;
                }

                index = index.saturating_sub(step);

                if index == 0 {
                    break;
                }
            }

            for height in indexes {
                let mut read_opts = ReadOptions::default();
                read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts());

                let key = <Self as Table>::encode_key(&height); // TODO: cleanup
                let hash = <Self as Table>::Value::decode_all(
                    &snapshot
                        .get_cf_opt(cf, &key, read_opts)
                        .map_err(Error::Rocks)?
                        .unwrap(), // TODO: error handle
                )?;

                out.push(Point {
                    height,
                    hash: BlockHash::from_byte_array(hash),
                });
            }
        }

        out.push(Point {
            height: 0,
            hash: genesis_hash,
        });

        Ok(out)
    }
}
