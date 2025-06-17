use std::collections::HashMap;

use gasket::framework::*;
use tokio::time::Instant;
use tracing::{error, info, warn};

use crate::{
    storage::{kv_store::StorageHandler, timestamp::Timestamp},
    sync::{
        Network,
        stages::{
            ChainEvent, Point, TransactionWithId,
            index::indexers::core::hash_by_height::HashByHeightKV,
        },
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
    pub fn new(node_address: &String, network: Network, db: StorageHandler) -> Self {
        Self {
            node_address: node_address.clone(),
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

        let peer_session = Peer::connect(&stage.node_address, stage.network.magic())
            .await
            .or_retry()?;

        info!(
            node = stage.node_address,
            network = ?stage.network,
            "connected to upstream node"
        );

        // get intersect options from KV store/rollback buffer

        let reader = stage.db.reader(Timestamp::from_u64(u64::MAX)); // TODO ts

        let intersect_options =
            HashByHeightKV::intersect_options(&reader, stage.network.genesis_block().block_hash())
                .or_panic()?;

        // TODO: if no rollback buffer/not mutable, download all the headers (from the current tip in
        // storage) to discover the chain tip (and thus the point at which to become mutable)

        Ok(Worker {
            peer_session,
            cursor: intersect_options,
            init: false,
            stats: PullStats::new(),
        })
    }

    async fn schedule(
        &mut self,
        stage: &mut Stage,
    ) -> Result<WorkSchedule<Vec<ChainEvent>>, WorkerError> {
        // TODO: timed stats

        let mut units = vec![];

        // send initial rollback if applicable
        if !self.init {
            if let Some(point) = self.cursor.first() {
                // TODO
                // if point.height != 0 {
                //     units.push(ChainEvent::RollBack(*point));
                // }
                self.init = true;
            }
        }

        // fetch new headers using cursor
        let mut headers = self
            .peer_session
            .get_new_headers(self.cursor.iter().map(|x| x.hash).collect())
            .await
            .or_restart()?;

        // fetch configurable amount of blocks at once
        headers.truncate(10); // TODO: config

        // if no new headers, wait for new block or do mempool refreshing ..
        while headers.is_empty() {
            self.peer_session.new_block_notification.notified().await;

            headers = self
                .peer_session
                .get_new_headers(self.cursor.iter().map(|x| x.hash).collect())
                .await
                .or_restart()?;
        }

        // fetch blocks for headers
        let blocks = self
            .peer_session
            .get_blocks(headers.iter().map(|x| x.block_hash()).collect())
            .await
            .or_restart()?;

        if headers.len() != blocks.len() {
            error!("header len/block len mismatch");
            return Err(WorkerError::Restart);
        }

        let headers = headers.into_iter().zip(blocks).collect::<Vec<_>>();

        let cursor_map: HashMap<_, _> = self
            .cursor
            .iter()
            .map(|point| (point.hash, point.height))
            .collect();

        let cursor_tip = self.cursor.first().map(|x| x.height);

        let headers = if let Some((first, _)) = headers.first() {
            // find the height of the first returned header using the cursor
            let intersect_height = cursor_map.get(&first.prev_blockhash).ok_or_else(|| {
                warn!(
                    "could not find intersect height for {:?}",
                    first.prev_blockhash
                );
                WorkerError::Restart
            })?;

            // send a rollback to the intersect if it was behind our tip
            if let Some(cursor_tip_height) = cursor_tip {
                if *intersect_height != cursor_tip_height {
                    info!("rollbacks unimplemented");
                    units.push(ChainEvent::RollBack(Point {
                        height: *intersect_height,
                        hash: first.prev_blockhash,
                    }))
                }
            }

            // assign heights to the received headers using the intersect height
            headers
                .into_iter()
                .zip((*intersect_height + 1)..)
                .collect::<Vec<_>>()
        } else {
            warn!(
                "no headers returned from peer with cursors: {:?}",
                self.cursor
            );
            return Ok(WorkSchedule::Idle);
        };

        // try use just processed headers as intersects
        self.cursor = headers
            .iter()
            .rev()
            .take(50)
            .map(|((header, _), height)| Point {
                height: *height,
                hash: header.block_hash(),
            })
            .collect::<Vec<_>>();

        // else fetch more from db if we dont get many
        if self.cursor.len() < 50 {
            let reader = stage.db.reader(Timestamp::from_u64(u64::MAX));

            let mut options = HashByHeightKV::intersect_options(
                &reader,
                stage.network.genesis_block().block_hash(), // TODO dont compute
            )
            .or_panic()?;

            if let Some(last) = self.cursor.last() {
                // don't have multiple points for same height
                options.retain(|point| point.height < last.height);
                self.cursor.extend(options)
            } else {
                self.cursor = options
            }

            // info!("got intersect options: {:?}", self.cursor)
        }

        self.cursor.push(Point {
            height: 0,
            hash: stage.network.genesis_block().block_hash(),
        }); // cleanup

        for ((header, block), height) in headers {
            let header_hash = header.block_hash();

            if header_hash != block.block_hash() {
                error!("header/block hash mismatch");
                return Err(WorkerError::Restart);
            }

            // if height > self.tip.0 {
            //     self.tip = (height, header.block_hash());
            // }

            let point = Point {
                height,
                hash: header_hash,
            };

            let block_txs = block
                .txdata
                .into_iter()
                .map(|tx| TransactionWithId {
                    tx_id: tx.compute_txid(),
                    tx,
                })
                .collect();

            units.push(ChainEvent::RollForward(point, header, block_txs));
        }

        Ok(WorkSchedule::Unit(units))
    }

    async fn execute(
        &mut self,
        unit: &Vec<ChainEvent>,
        stage: &mut Stage,
    ) -> Result<(), WorkerError> {
        for u in unit {
            self.process_action(stage, u).await.or_panic()?;
            self.stats.unit_processed();
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
    stats: PullStats,
}

impl Worker {
    async fn send(&mut self, stage: &mut Stage, event: ChainEvent) -> Result<(), WorkerError> {
        stage
            .downstream
            .send(event.clone().into())
            .await
            .or_panic()?;

        // TODO
        // stage
        //     .health_downstream
        //     .send(event.into())
        //     .await
        //     .or_panic()?;

        Ok(())
    }

    async fn process_action(
        &mut self,
        stage: &mut Stage,
        next: &ChainEvent,
    ) -> Result<(), WorkerError> {
        match next {
            p @ ChainEvent::RollForward(_point, ..) => {
                // info!(?point, "pull roll forward");

                self.send(stage, p.clone().into()).await.or_panic()?;

                Ok(())
            }
            p @ ChainEvent::RollBack(_point) => {
                // info!(?point, "pull rollback");

                self.send(stage, p.clone().into()).await.or_panic()?;

                Ok(())
            }
            p @ ChainEvent::MempoolBlocks(_info, ..) => {
                // info!(?info, "pull mempool blocks");

                self.send(stage, p.clone().into()).await.or_panic()?;

                Ok(())
            }
        }
    }
}

// TODO: improve
pub struct PullStats {
    processed: usize,
    last_checkpoint: Instant,
}

impl PullStats {
    pub fn new() -> Self {
        Self {
            processed: 0,
            last_checkpoint: Instant::now(),
        }
    }

    pub fn unit_processed(&mut self) {
        self.processed += 1;

        if self.processed % 1000 == 0 {
            let time_taken = self.last_checkpoint.elapsed();

            info!(
                "last 1000 units in {time_taken:?} ({} u/s)",
                1000 as f64 / time_taken.as_secs_f64()
            );
        }
    }
}
