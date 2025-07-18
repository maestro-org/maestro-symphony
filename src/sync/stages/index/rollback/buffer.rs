use std::collections::{HashMap, HashSet, VecDeque};

use bitcoin::{BlockHash, hashes::Hash};
use tracing::{info, warn};

use crate::{
    error::Error,
    storage::{
        kv_store::{PreviousValue, RawKey, StorageHandler},
        table::Table,
        timestamp::Timestamp,
    },
    sync::{
        Config,
        stages::{Point, index::indexers::core::rollback_buffer::RollbackBufferKV},
    },
};

pub const DEFAULT_MAX_ROLLBACK: usize = 16;

#[derive(Debug, Clone)]
pub struct RollbackBuffer {
    points: VecDeque<PointWithOriginalKVs>,
    size: usize,
    safe_mode: bool,
}

#[derive(Debug, Clone)]
pub struct PointWithOriginalKVs {
    pub point: Point,
    pub original_kvs: HashMap<RawKey, PreviousValue>,
}

/// If we found the given point in the buffer return all the blocks which came
/// after that point, starting with the most recent, otherwise reflect that the
/// point was not found
#[derive(Debug)]
pub enum RollbackResult {
    PointFound(Vec<PointWithOriginalKVs>),
    PointNotFound,
}

impl RollbackBuffer {
    pub fn new(size: usize, safe_mode: bool) -> Self {
        Self {
            points: VecDeque::new(),
            size: size + 2, // +1 to include rb point, +1 to include mempool psuedo point
            safe_mode,
        }
    }

    pub fn capacity(&self) -> usize {
        self.size
    }

    /// Find the position of a point within the buffer
    pub fn position(&self, point: &Point) -> Option<usize> {
        self.points.iter().position(|p| p.point.eq(point))
    }

    /// Returns the number of blocks in the buffer
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn latest(&self) -> Option<&PointWithOriginalKVs> {
        self.points.front()
    }

    pub fn oldest(&self) -> Option<&PointWithOriginalKVs> {
        self.points.back()
    }

    /// Add a new block to the front of the rollback buffer and pop the oldest
    /// block if the length of the buffer is MAX_BUFFER_LEN.
    pub fn add_block(&mut self, point: Point, original_kvs: HashMap<RawKey, PreviousValue>) {
        self.points.push_front(PointWithOriginalKVs {
            point,
            original_kvs,
        });
        self.points.truncate(self.size);
    }

    /// Return an vector of the blocks which have been processed since the
    /// given point and remove those blocks from the buffer (most recent first)
    pub fn rollback_to_point(&mut self, point: &Point) -> Result<Vec<PointWithOriginalKVs>, Error> {
        match self.position(point) {
            Some(p) => Ok(self.points.drain(..p).collect()),
            None => Err(Error::Rollback(*point)),
        }
    }

    /// Like rollback to point but does not remove the points
    pub fn points_since(&self, point: &Point) -> Result<Vec<PointWithOriginalKVs>, Error> {
        match self.position(point) {
            Some(p) => Ok(self.points.range(..p).cloned().collect()),
            None => Err(Error::Rollback(*point)),
        }
    }

    /// insert new original kvs for a point into the rollback buffer
    pub fn insert_actions_for_point(
        &mut self,
        point: &Point,
        new_actions: HashMap<RawKey, PreviousValue>,
    ) {
        match self.position(point) {
            Some(p) => {
                let entry = self.points.get_mut(p).unwrap();

                if self.safe_mode {
                    for (key, original) in new_actions.iter() {
                        if entry.original_kvs.keys().any(|k| k == key) {
                            panic!(
                                "inserting action for key already present in rb buf {key:?} -> {original:?}"
                            )
                        }
                    }
                }

                entry.original_kvs.extend(new_actions);
            }
            None => self.add_block(*point, new_actions),
        }
    }

    pub fn remove_actions_for_point(&mut self, point: &Point, remove_keys: HashSet<RawKey>) {
        match self.position(point) {
            Some(p) => {
                let entry = self.points.get_mut(p).unwrap();

                if self.safe_mode {
                    for key in remove_keys.iter() {
                        if !entry.original_kvs.contains_key(key) {
                            panic!("trying to remove inverse action which doesn't exist {key:?}");
                        }
                    }
                }

                // keep only actions which we are not removing
                entry.original_kvs.retain(|k, _| !remove_keys.contains(k));

                // remove if empty
                if entry.original_kvs.is_empty() {
                    info!("removing point from rb buf as all actions removed");
                    self.points.remove(p);
                }
            }
            None => warn!("no point found when trying to remove"),
        }
    }
}

impl RollbackBuffer {
    pub fn fetch_from_storage(config: &Config, db: &StorageHandler) -> Result<Self, Error> {
        let mut rollback_buffer = RollbackBuffer::new(
            config.max_rollback.unwrap_or(DEFAULT_MAX_ROLLBACK),
            config.safe_mode.unwrap_or_default(),
        );

        let range = <RollbackBufferKV>::encode_range(None::<&()>, None::<&()>);

        let reader = db.reader(Timestamp::from_u64(u64::MAX)); // TODO ts

        let iter = reader.iter_kvs::<RollbackBufferKV>(range, false);

        for kv in iter {
            let (k, v) = kv?;

            let point = Point {
                height: k.height,
                hash: BlockHash::from_byte_array(k.hash),
            };

            let action = vec![(k.key, v)].into_iter().collect();

            rollback_buffer.insert_actions_for_point(&point, action);
        }

        Ok(rollback_buffer)
    }
}
