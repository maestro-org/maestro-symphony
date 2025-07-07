use crate::{
    error::Error,
    storage::{
        kv_store::{PreviousValue, StorageHandler},
        table::Table,
        timestamp::Timestamp,
    },
    sync::{
        self, IndexersConfig,
        stages::{
            ChainEvent, Point,
            index::{
                indexers::{
                    core::{
                        hash_by_height::HashByHeightKV,
                        indexer_info::{IndexerInfo, IndexerInfoKV},
                        rollback_buffer::{RollbackBufferKV, RollbackKey},
                        utxo_by_txo_ref::UtxoCache,
                    },
                    custom::id::ProcessTransaction,
                },
                rollback::buffer::{DEFAULT_MAX_ROLLBACK, RollbackBuffer},
                worker::{context::IndexingContext, get_default_cache_size},
            },
        },
    },
};
use bitcoin::{BlockHash, hashes::Hash};
use gasket::framework::*;
use rocksdb::WriteBatch;
use tracing::{debug, error, info, warn};

/*
    Index Stage

    After receiving a new block, estimated mempool blocks or a rollback instruction from the Pull
    stage, the indexing stage is responsible for running core indexing (like storing new UTxOs) and
    the other reducers, and writing to storage.
*/

pub type UpstreamPort = gasket::messaging::tokio::InputPort<ChainEvent>;
pub type DownstreamPort = gasket::messaging::tokio::OutputPort<()>;

#[derive(Stage)]
#[stage(name = "index", unit = "ChainEvent", worker = "Worker")]
pub struct Stage {
    db: StorageHandler,
    indexers: IndexersConfig,
    network: sync::Network,
    rollback_buffer: RollbackBuffer,
    max_rollbacks: u64,
    // tip of our processed chain in db (not including mempool)
    last_processed: Point,
    // does our processed chain in db reflect mempool blocks (TODO)
    processed_mempool: bool,
    utxo_cache: Option<UtxoCache>,

    pub upstream: UpstreamPort,
    pub downstream: DownstreamPort,
}

impl Stage {
    pub fn new(config: sync::Config, db: StorageHandler) -> Result<Self, Error> {
        let utxo_cache_size = config
            .utxo_cache_size
            .unwrap_or_else(|| get_default_cache_size());

        let utxo_cache = if utxo_cache_size == 0 {
            None
        } else {
            info!("using utxo cache with size {utxo_cache_size}");
            Some(UtxoCache::new(utxo_cache_size))
        };

        // TODO: in worker vs stage?
        let rollback_buffer = RollbackBuffer::fetch_from_storage(&config, &db)?;

        let storage = db.reader(Timestamp::from_u64(u64::MAX)); // TODO

        // TODO: helper
        let range = HashByHeightKV::encode_range(None::<&()>, None::<&()>);

        let res = storage
            .iter_kvs::<HashByHeightKV>(range, true)
            .next()
            .transpose()?;

        let last_processed = if let Some((height, hash)) = res {
            Point {
                height,
                hash: BlockHash::from_byte_array(hash),
            }
        } else {
            Point {
                height: 0,
                hash: config.network.genesis_block().block_hash(),
            }
        };

        let indexer_info = storage.get::<IndexerInfoKV>(&())?;

        let processed_mempool = indexer_info.as_ref().map(|x| x.mempool).unwrap_or(false);

        if let Some(info) = indexer_info {
            let info_cursor = Point {
                height: info.last_point_height,
                hash: BlockHash::from_byte_array(info.last_point_hash),
            };

            if let Some(rbbuf_latest) = rollback_buffer.latest() {
                if info.mempool {
                    assert_eq!(rbbuf_latest.point.hash, info_cursor.hash);
                    assert_eq!(rbbuf_latest.point.hash, last_processed.hash);
                } else {
                    assert_eq!(rbbuf_latest.point, last_processed);
                    assert_eq!(rbbuf_latest.point, info_cursor);
                }
            }
        }

        info!(
            "starting indexer stage (last processed: {last_processed:?}, rollback buffer len: {})",
            rollback_buffer.len()
        );

        Ok(Self {
            db,
            network: config.network,
            indexers: config.indexers,
            rollback_buffer,
            max_rollbacks: config.max_rollback.unwrap_or(DEFAULT_MAX_ROLLBACK) as u64,
            last_processed,
            processed_mempool,
            utxo_cache,

            upstream: Default::default(),
            downstream: Default::default(),
        })
    }

    fn rollback_to_point(&mut self, rb_point: &Point) -> Result<(), WorkerError> {
        let blocks_to_undo = self.rollback_buffer.points_since(rb_point).or_panic()?;

        // TODO refactor
        let mut wb = WriteBatch::new();

        let commit_ts = self
            .db
            .previous_timestamp
            .clone()
            .map(|x| Timestamp::after(x))
            .unwrap_or(Timestamp::new());

        let ts = commit_ts.as_rocksdb_ts();

        let cf = self.db.cf_handle();

        for block in blocks_to_undo {
            let point = block.point;

            for (key, original_kv) in block.original_kvs {
                // perform inverse action
                match original_kv {
                    PreviousValue::Present(prev) => wb.put_cf_with_ts(cf, &key, ts, prev),
                    PreviousValue::NotPresent => wb.delete_cf_with_ts(cf, &key, ts),
                }

                // remove entry from persistent rollback buffer
                let rollback_key = RollbackBufferKV::encode_key(&RollbackKey {
                    height: point.height,
                    hash: point.hash.to_byte_array(),
                    key: key.clone(),
                });

                wb.delete_cf_with_ts(cf, rollback_key, ts);
            }
        }

        // TODO: just update ts here instead of using everywhere above?
        self.db.db.write(wb).or_restart()?;
        self.db.previous_timestamp = Some(commit_ts);

        self.last_processed = *rb_point;
        self.processed_mempool = false;

        self.rollback_buffer
            .rollback_to_point(rb_point)
            .or_panic()?;

        Ok(())
    }
}

pub struct Worker {
    indexers: Vec<Box<dyn ProcessTransaction>>,
    mutable: bool,
}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        let indexers = stage
            .indexers
            .transaction_indexers
            .clone()
            .into_iter()
            .map(|spec| spec.create_indexer())
            .collect::<Result<Vec<_>, _>>()
            .or_panic()?;

        Ok(Worker {
            indexers,
            mutable: !stage.rollback_buffer.is_empty(),
        })
    }

    async fn schedule(
        &mut self,
        stage: &mut Stage,
    ) -> Result<WorkSchedule<ChainEvent>, WorkerError> {
        let event = stage.upstream.recv().await.or_panic()?.payload;

        Ok(WorkSchedule::Unit(event))
    }

    async fn execute(&mut self, unit: &ChainEvent, stage: &mut Stage) -> Result<(), WorkerError> {
        match unit {
            ChainEvent::RollForward(point, header, txs, tip) => {
                // undo mempool blocks if processed
                if stage.processed_mempool {
                    info!("undoing mempool blocks before processing new confirmed block");
                    stage.rollback_to_point(&(stage.last_processed.clone()))?;
                };

                /* check received block against processed chain */

                let expected_height = stage.last_processed.height + 1;
                if point.height != expected_height {
                    // TODO: if pull stage panics, and there are blocks in the pull -> index queue, we may receive
                    warn!(
                        "received roll forward for point {}:{} as expecting height {expected_height}...",
                        point.height, point.hash
                    );
                    return Ok(());
                }

                let expected_prev_bh = stage.last_processed.hash;
                if header.prev_blockhash != expected_prev_bh {
                    error!("previous block hash mismatch");
                    return Err(WorkerError::Panic);
                }

                /* start maintaining a rollback buffer, if we don't already, when we are
                sufficiently close to the chain tip */

                self.mutable |= point.height > (tip.height.saturating_sub(stage.max_rollbacks));

                /* index the block */

                info!("indexing {point:?} (mutable: {})...", self.mutable);

                let mut task = stage.db.begin_indexing_task(self.mutable);

                let mut ctx = IndexingContext::new(
                    &mut task,
                    txs,
                    *point,
                    stage.network,
                    &mut stage.utxo_cache,
                )
                .or_restart()?;

                for (block_index, tx) in txs.iter().enumerate() {
                    for indexer in &self.indexers {
                        indexer
                            .process_tx(&mut task, tx, block_index, &mut ctx)
                            .or_restart()?;
                    }

                    ctx.update_utxo_set(&mut task, tx, &mut stage.utxo_cache)
                        .or_restart()?;
                }

                IndexerInfoKV::set_info(
                    &mut task,
                    IndexerInfo::new(point.height, point.hash.to_byte_array(), false),
                )
                .or_restart()?;

                task.set::<HashByHeightKV>(point.height, point.hash.to_byte_array())
                    .or_restart()?;

                // TODO: block-level indexers (might have to change utxo resolver removal to after)

                let task = task.finalize();

                let original_kvs = task.original_kvs.clone();

                stage
                    .db
                    .apply_indexing_task(task, point, stage.max_rollbacks, None)
                    .or_restart()?;

                stage.last_processed = *point;

                // TODO, move into stage.apply_task or something?
                if let Some(original_kvs) = original_kvs {
                    stage.rollback_buffer.add_block(*point, original_kvs);
                }
            }
            ChainEvent::RollBack(rb_point) => {
                info!("rolling back to {rb_point:?}...");

                stage.rollback_to_point(rb_point)?
            }
            ChainEvent::MempoolBlocks(info, mempool_blocks) => {
                // -- first rollback previous mempool blocks

                let true_tip = stage.last_processed.clone();

                let blocks_to_undo = stage.rollback_buffer.points_since(&true_tip).or_panic()?;

                if !blocks_to_undo.is_empty() {
                    debug!("undoing previous mempool blocks");
                    stage.rollback_to_point(&true_tip)?
                }

                // --

                // check mempool blocks were built/chained upon our current tip, else skip
                if info.tip != stage.last_processed.hash {
                    info!(
                        "skipping mempool snapshot chained on {} as our tip is {}",
                        info.tip, stage.last_processed.hash
                    );
                    return Ok(());
                }

                info!(
                    "indexing {} mempool blocks (snapshot: {})...",
                    mempool_blocks.len(),
                    info.timestamp
                );

                let mut task = stage.db.begin_mempool_indexing_task();

                let mempool_txs = mempool_blocks.iter().cloned().flatten().collect();

                let mut mempool_tip_height = stage.last_processed.height;

                let mut ctx = IndexingContext::new(
                    &mut task,
                    &mempool_txs,
                    stage.last_processed,
                    stage.network,
                    &mut stage.utxo_cache,
                )
                .or_restart()?;

                for (i, txs) in mempool_blocks.iter().enumerate() {
                    mempool_tip_height = stage.last_processed.height + 1 + i as u64;

                    let pseudo_point = Point {
                        height: mempool_tip_height,
                        hash: stage.last_processed.hash,
                    };

                    // we will use the same indexing context, but we need to update the point
                    ctx.update_point(pseudo_point);

                    for (block_index, tx) in txs.iter().enumerate() {
                        for indexer in &self.indexers {
                            indexer
                                .process_tx(&mut task, tx, block_index, &mut ctx)
                                .or_restart()?;
                        }

                        ctx.update_utxo_set(&mut task, tx, &mut stage.utxo_cache)
                            .or_restart()?;
                    }

                    IndexerInfoKV::set_info(
                        &mut task,
                        IndexerInfo::new(
                            pseudo_point.height,
                            pseudo_point.hash.to_byte_array(),
                            // Some(info.timestamp),
                            true,
                        ),
                    )
                    .or_restart()?;
                }

                // TODO: block-level indexers (might have to change utxo resolver removal to after)

                let task = task.finalize();

                let original_kvs = task.original_kvs.clone();

                let pseudo_point = Point {
                    height: mempool_tip_height,
                    hash: stage.last_processed.hash,
                };

                stage
                    .db
                    .apply_indexing_task(
                        task,
                        &pseudo_point,
                        stage.max_rollbacks,
                        Some(info.timestamp),
                    )
                    .or_restart()?;

                stage.processed_mempool = true;

                // TODO, move into stage.apply_task or something?
                if let Some(original_kvs) = original_kvs {
                    let mempool_rbbuf_point = Point {
                        height: u64::MAX,
                        hash: stage.last_processed.hash,
                    };

                    stage
                        .rollback_buffer
                        .add_block(mempool_rbbuf_point, original_kvs);
                }

                // TODO delta refresh
            }
        };

        Ok(())
    }

    async fn teardown(&mut self) -> Result<(), WorkerError> {
        Ok(())
    }
}
