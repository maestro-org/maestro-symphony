use std::collections::HashMap;

use bitcoin::hashes::Hash;
use gasket::framework::*;
use serde::Deserialize;
use tracing::{debug, info};

use crate::{
    storage::{kv_store::StorageHandler, timestamp::Timestamp},
    sync::stages::{
        ChainEvent,
        index::{
            indexers::{
                core::utxo_by_txo_ref::{TxoRef, Utxo},
                custom::{TransactionIndexerFactory, id::ProcessTransaction},
            },
            worker::context::IndexingContext,
        },
    },
};

/*
    Index Stage

    After receiving a new block, estimated mempool blocks or a rollback instruction from the Pull
    stage, the indexing stage is responsible for running core indexing (like storing new UTxOs) and
    the other reducers, and writing to storage.
*/

pub type UpstreamPort = gasket::messaging::tokio::InputPort<ChainEvent>;
pub type DownstreamPort = gasket::messaging::tokio::OutputPort<()>;

#[derive(Debug, Deserialize)]
pub struct StageConfig {
    #[serde(default)]
    pub transaction_indexers: Vec<TransactionIndexerFactory>,
}

#[derive(Stage)]
#[stage(name = "index", unit = "ChainEvent", worker = "Worker")]
pub struct Stage {
    db: StorageHandler,
    config: StageConfig,

    // custom indexers
    upstream: UpstreamPort,
    downstream: DownstreamPort,
}

impl Stage {
    pub fn new(db: StorageHandler, config: StageConfig) -> Self {
        Self {
            db,
            config,

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
        // TODO: wipe db task

        let indexers = stage
            .config
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

        let mut task = stage.db.begin_task(mutable);

        let result = match unit {
            ChainEvent::RollForward(point, _header, txs) => {
                let mut ctx = IndexingContext::new(&mut task, txs, *point).or_restart()?;

                for (block_index, tx) in txs.iter().enumerate() {
                    for indexer in &self.indexers {
                        indexer
                            .process_tx(&mut task, tx, block_index, &ctx)
                            .or_restart()?;
                    }

                    ctx.update_utxo_set(&mut task, tx).or_restart()?;
                }

                // block indexers (might have to change utxo resolver removal to after)

                // cursor, rb buf, ...
            }
            ChainEvent::RollBack(point) => {
                // find point in rollback buffer

                // apply all inverse actions (merged) and trim rb buff

                // re-insert utxos which are no longer spent, delete produced utxos

                // timestamp entry

                unimplemented!()
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

        let task = task.finalize();
        stage.db.apply_task(task).or_restart()?;

        Ok(())
    }

    async fn teardown(&mut self) -> Result<(), WorkerError> {
        Ok(())
    }
}
