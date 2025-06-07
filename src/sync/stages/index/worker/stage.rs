use crate::{
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
                        rollback_buffer::{RollbackBufferKV, RollbackKey},
                    },
                    custom::id::ProcessTransaction,
                },
                rollback::buffer::{DEFAULT_MAX_BUFFER_LEN, RollbackBuffer},
                worker::context::IndexingContext,
            },
        },
    },
};
use bitcoin::{BlockHash, hashes::Hash};
use gasket::framework::*;
use rocksdb::WriteBatch;
use tracing::info;

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
    safe_mode: bool,

    // custom indexers
    pub upstream: UpstreamPort,
    pub downstream: DownstreamPort,
}

impl Stage {
    pub fn new(config: sync::Config, db: StorageHandler) -> Self {
        let safe_mode = config.safe_mode.unwrap_or_default();

        // -- populate in memory buffer using persistent rb buf

        let mut rollback_buffer = RollbackBuffer::new(
            config.max_rollback.unwrap_or(DEFAULT_MAX_BUFFER_LEN),
            safe_mode,
        );

        // TODO: cleanup
        {
            let snapshot = db.db.snapshot();

            let range = <RollbackBufferKV>::encode_range(None::<&()>, None::<&()>);

            let iter = db.iter_kvs::<RollbackBufferKV>(
                &snapshot,
                range,
                Timestamp::from_u64(u64::MAX),
                false,
            );

            for kv in iter {
                let (k, v) = kv.unwrap();

                let point = Point {
                    height: k.height,
                    hash: BlockHash::from_byte_array(k.hash),
                };

                let action = vec![(k.key, v)].into_iter().collect();

                rollback_buffer.insert_actions_for_point(&point, action);
            }
        }

        Self {
            db,
            network: config.network,
            indexers: config.indexers,
            rollback_buffer,
            safe_mode,

            upstream: Default::default(),
            downstream: Default::default(),
        }
    }
}

pub struct Worker {
    indexers: Vec<Box<dyn ProcessTransaction>>,
}

impl Worker {}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        // TODO wipe live task
        // stage.db.task_live = false;

        let indexers = stage
            .indexers
            .transaction_indexers
            .clone()
            .into_iter()
            .map(|spec| spec.create_indexer())
            .collect::<Result<Vec<_>, _>>()
            .or_panic()?;

        Ok(Worker { indexers })
    }

    async fn schedule(
        &mut self,
        stage: &mut Stage,
    ) -> Result<WorkSchedule<ChainEvent>, WorkerError> {
        let event = stage.upstream.recv().await.or_panic()?.payload;

        Ok(WorkSchedule::Unit(event))
    }

    async fn execute(&mut self, unit: &ChainEvent, stage: &mut Stage) -> Result<(), WorkerError> {
        let mutable = false; // TODO

        match unit {
            ChainEvent::RollForward(point, _header, txs) => {
                let mut task = stage.db.begin_task(mutable);

                info!("indexing {point:?}...");

                let mut ctx =
                    IndexingContext::new(&mut task, txs, *point, stage.network).or_restart()?;

                for (block_index, tx) in txs.iter().enumerate() {
                    for indexer in &self.indexers {
                        indexer
                            .process_tx(&mut task, tx, block_index, &mut ctx)
                            .or_restart()?;
                    }

                    ctx.update_utxo_set(&mut task, tx).or_restart()?;
                }

                // IndexerInfoKV::set_info(&mut task, IndexerInfo::new()).or_restart()?; // TODO

                task.set::<HashByHeightKV>(point.height, point.hash.to_byte_array())
                    .or_restart()?;

                // block indexers (might have to change utxo resolver removal to after)

                // cursor?

                // TODO: gc persistent buffer entries

                let task = task.finalize();

                let original_kvs = task.original_kvs.clone();

                stage.db.apply_task(task, point).or_restart()?;
                stage.db.db.flush().unwrap();

                // TODO, move into stage.apply_task or something
                if let Some(original_kvs) = original_kvs {
                    stage.rollback_buffer.add_block(*point, original_kvs);
                }
            }
            ChainEvent::RollBack(rb_point) => {
                // TODO refactor
                let mut wb = WriteBatch::new();

                let commit_ts = stage
                    .db
                    .previous_timestamp
                    .clone()
                    .map(|x| Timestamp::after(x))
                    .unwrap_or(Timestamp::new());

                let ts = commit_ts.as_rocksdb_ts();

                let cf = stage.db.cf_handle();

                let blocks_to_undo = stage.rollback_buffer.points_since(rb_point).or_panic()?;

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

                stage.db.db.write(wb).or_restart()?;
                stage.db.previous_timestamp = Some(commit_ts);

                stage
                    .rollback_buffer
                    .rollback_to_point(rb_point)
                    .or_panic()?;
            }
            ChainEvent::MempoolBlocks(info, mempool_blocks) => {
                // resolve UTxOs

                // cache the resolved utxos between refreshes as well as the actions

                // create merged storage actions for all the mempool blocks

                // delta refresh

                // timestamp entry

                unimplemented!()
            }
        };

        Ok(())
    }

    async fn teardown(&mut self) -> Result<(), WorkerError> {
        Ok(())
    }
}
