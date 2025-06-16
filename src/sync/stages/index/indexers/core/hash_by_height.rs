use crate::storage::kv_store::Reader;
use crate::storage::table::Table;
use crate::sync::stages::BlockHash;
use bitcoin::hashes::Hash;

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
        reader: &Reader,
        genesis_hash: BlockHash,
    ) -> Result<Vec<Point>, Error> {
        let mut out = vec![];

        // scan entire table
        let range = <Self>::encode_range(None::<&()>, None::<&()>);

        let mut iter = reader.iter_kvs::<Self>(range, true);

        let last_entry_height = match iter.next() {
            Some(res) => {
                let (height, _) = res?;
                Some(height)
            }
            None => None,
        };

        let mut indexes = vec![];

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
                let hash = reader
                    .get::<Self>(&height)?
                    .expect("missing hash by height kv");

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
