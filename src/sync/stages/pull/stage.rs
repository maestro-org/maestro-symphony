use gasket::framework::*;
use tracing::info;

use crate::{
    storage::kv_store::StorageHandler,
    sync::{
        Network,
        stages::{ChainEvent, Point},
    },
};

use super::peer::Peer;

/*
    Pull Stage

    Responsible for talking to a node in order to discover new blocks, mempool transactions and
    rollbacks and then passing them downstream to the indexing stage.
*/

pub type DownstreamPort = gasket::messaging::tokio::OutputPort<ChainEvent>;

#[derive(Stage)]
#[stage(name = "pull", unit = "Vec<ChainEvent>", worker = "Worker")]
pub struct Stage {
    node_address: String,
    network: Network,

    db: StorageHandler,

    pub downstream: DownstreamPort,
    pub health_downstream: DownstreamPort,
}

impl Stage {
    pub fn new(node_address: String, network: Network, db: StorageHandler) -> Self {
        Self {
            node_address,
            network,
            db,
            downstream: Default::default(),
            health_downstream: Default::default(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        info!("connecting to node {}...", stage.node_address);

        let _peer_session = Peer::connect(&stage.node_address, stage.network.magic())
            .await
            .or_retry()?;

        info!(
            node = stage.node_address,
            network = ?stage.network,
            "connected to upstream node"
        );

        // get intersect options from KV store/rollback buffer

        // if no rollback buffer/not mutable, download all the headers (from the current tip in
        // storage) to discover the chain tip (and thus the point at which to become mutable)

        unimplemented!()
    }

    async fn schedule(
        &mut self,
        stage: &mut Stage,
    ) -> Result<WorkSchedule<Vec<ChainEvent>>, WorkerError> {
        // in initial

        // (timed stats)

        // send rollback to intersect point if initial start up

        // fetch new headers using cursor

        // if no new headers, wait for new block or do mempool refreshing ..

        // fetch blocks for headers

        // detect rollback by comparing received headers

        // add roll forwards actions to unit

        unimplemented!()
    }

    async fn execute(
        &mut self,
        unit: &Vec<ChainEvent>,
        stage: &mut Stage,
    ) -> Result<(), WorkerError> {
        for u in unit {
            self.process_action(stage, u).await.or_panic()?;
        }

        Ok(())
    }

    async fn teardown(&mut self) -> Result<(), WorkerError> {
        self.peer_session.handler.abort();

        Ok(())
    }
}

pub struct Worker {
    peer_session: Peer,
    cursor: Vec<Point>,
    init: bool,
}

impl Worker {
    async fn send(&mut self, stage: &mut Stage, event: ChainEvent) -> Result<(), WorkerError> {
        stage
            .downstream
            .send(event.clone().into())
            .await
            .or_panic()?;

        stage
            .health_downstream
            .send(event.into())
            .await
            .or_panic()?;

        Ok(())
    }

    async fn process_action(
        &mut self,
        stage: &mut Stage,
        next: &ChainEvent,
    ) -> Result<(), WorkerError> {
        match next {
            p @ ChainEvent::RollForward(point, ..) => {
                info!(?point, "pull roll forward");

                self.send(stage, p.clone().into()).await.or_panic()?;

                Ok(())
            }
            p @ ChainEvent::RollBack(point) => {
                info!(?point, "pull rollback");

                self.send(stage, p.clone().into()).await.or_panic()?;

                Ok(())
            }
            p @ ChainEvent::MempoolBlocks(info, ..) => {
                info!(?info, "pull mempool blocks");

                self.send(stage, p.clone().into()).await.or_panic()?;

                Ok(())
            }
        }
    }
}
