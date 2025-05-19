use std::collections::HashMap;

use gasket::framework::*;
use tracing::{debug, info};

use crate::{storage::kv_store::StorageHandler, sync::stages::ChainEvent};

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

    // custom indexers
    upstream: UpstreamPort,
    downstream: DownstreamPort,
}

impl Stage {
    pub fn new(db: StorageHandler) -> Self {
        Self {
            db,

            upstream: Default::default(),
            downstream: Default::default(),
        }
    }
}

pub struct Worker {}

impl Worker {}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        unimplemented!()
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
            ChainEvent::RollForward(point, header, txs) => {
                let mutable = false; // TODO

                let dbtx = stage.db.begin_task(mutable);

                // run core pre-indexing (utxo insertion?)

                // resolve UTxOs

                // run all reducers

                // run core post-indexing (utxo removal?)

                // finish task

                unimplemented!();
            }
            ChainEvent::RollBack(point) => {
                // find point in rollback buffer

                // apply all inverse actions (merged) and trim rb buff

                // re-insert utxos which are no longer spent, delete produced utxos

                // timestamp entry

                unimplemented!()
            }
            ChainEvent::MempoolBlocks(info, mempool_blocks) => {
                // cache the resolved utxos between refreshes as well as the actions

                // create merged storage actions for all the mempool blocks

                // delta refresh

                // timestamp entry

                unimplemented!()
            }
        }
    }

    async fn teardown(&mut self) -> Result<(), WorkerError> {
        Ok(())
    }
}
