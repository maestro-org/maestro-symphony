use std::{collections::HashMap, time::Duration};

use bitcoin::{BlockHash, Transaction};
use bitcoincore_rpc::{
    Auth as RpcAuth, Client as RpcClient, RpcApi,
    json::{GetBlockTemplateModes, GetBlockTemplateRules},
};
use gasket::framework::*;
use tokio::{
    sync::{mpsc, oneshot::Receiver},
    time::{Instant, timeout},
};
use tracing::{error, info, warn};

use crate::{
    error::Error,
    storage::{kv_store::StorageHandler, timestamp::Timestamp},
    sync::{
        Network,
        stages::{
            ChainEvent, MempoolSnapshotInfo, Point, TransactionWithId,
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

pub type DownstreamPort = gasket::messaging::OutputPort<ChainEvent>;

#[derive(Stage)]
#[stage(name = "pull", unit = "Vec<ChainEvent>", worker = "Worker")]
pub struct Stage {
    node_p2p_address: String,
    node_rpc_address: Option<String>,
    node_rpc_auth: RpcAuth,
    network: Network,
    mempool_enabled: bool,
    intersect: Option<Point>,

    db: StorageHandler,

    should_shutdown: Option<Receiver<()>>,
    has_shutdown: Option<mpsc::Sender<()>>,
    tx_submission_recv: Option<mpsc::UnboundedReceiver<Transaction>>,

    pub downstream: DownstreamPort,
    pub health_downstream: DownstreamPort,
}

impl Stage {
    pub fn new(
        node_p2p_address: String,
        node_rpc_address: Option<String>,
        node_rpc_auth: RpcAuth,
        network: Network,
        mempool_enabled: bool,
        intersect: Option<Point>,
        db: StorageHandler,
        shutdown_signals: Option<(Receiver<()>, mpsc::Sender<()>)>,
        tx_submission_recv: Option<mpsc::UnboundedReceiver<Transaction>>,
    ) -> Self {
        let (should_shutdown, has_shutdown) = match shutdown_signals {
            Some((x, y)) => (Some(x), Some(y)),
            None => (None, None),
        };

        Self {
            node_p2p_address,
            node_rpc_address,
            node_rpc_auth,
            network,
            mempool_enabled,
            intersect,
            db,
            should_shutdown,
            has_shutdown,
            tx_submission_recv,
            downstream: Default::default(),
            health_downstream: Default::default(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        info!("connecting to node {}...", stage.node_p2p_address);

        let mut peer_session = Peer::connect(&stage.node_p2p_address, stage.network.magic())
            .await
            .or_retry()?;

        info!(
            node = stage.node_p2p_address,
            network = ?stage.network,
            "connected to upstream node"
        );

        // get intersect options from KV store/rollback buffer

        let reader = stage.db.reader(Timestamp::from_u64(u64::MAX)); // TODO ts

        // TODO move into stage? so that we dont send duplicate chain actions
        let intersect_options = HashByHeightKV::intersect_options(
            &reader,
            stage.network.genesis_block().block_hash(),
            stage.intersect,
        )
        .or_panic()?;

        let mut peer_rpc = if let Some(address) = stage.node_rpc_address.clone() {
            Some(RpcClient::new(&address, stage.node_rpc_auth.clone()).or_panic()?)
        } else {
            None
        };

        let tip = if let Some(rpc) = peer_rpc.as_mut() {
            let tip = rpc
                .get_chain_tips()
                .or_restart()?
                .into_iter()
                .max_by_key(|x| x.height)
                .ok_or_else(|| {
                    error!("No chaintip found");
                    WorkerError::Restart
                })?;

            Point {
                height: tip.height,
                hash: tip.hash,
            }
        } else {
            let tip_estimate = estimate_tip(&mut peer_session, *intersect_options.first().unwrap())
                .await
                .or_panic()?;

            info!("got tip estimate {tip_estimate:?}");

            tip_estimate
        };

        info!("bootstrapped pull stage (tip: {tip:?})");

        Ok(Worker {
            peer_session,
            peer_rpc,
            cursor: intersect_options,
            stats: PullStats::new(),
            tip_reached: false,
            has_shutdown: stage.has_shutdown.clone(),
            initialised: false,
            tip,
        })
    }

    async fn schedule(
        &mut self,
        stage: &mut Stage,
    ) -> Result<WorkSchedule<Vec<ChainEvent>>, WorkerError> {
        if stage
            .should_shutdown
            .as_mut()
            .map(|x| x.try_recv().is_ok())
            .unwrap_or_default()
        {
            info!("sync received shutdown signal");
            return Ok(WorkSchedule::Done);
        };

        if !self.initialised {
            if node_syncing(
                &mut self.peer_session,
                stage.network.genesis_block().block_hash(),
            )
            .await
            .or_restart()?
            {
                info!("it seems the node is still syncing, waiting 60s before retrying...");

                // // refresh tip
                // let tip = self
                //     .peer_rpc
                //     .m
                //     .get_chain_tips()
                //     .or_restart()?
                //     .into_iter()
                //     .max_by_key(|x| x.height)
                //     .ok_or_else(|| {
                //         error!("No chaintip found");
                //         WorkerError::Restart
                //     })?;

                // self.tip = Point {
                //     height: tip.height,
                //     hash: tip.hash,
                // };

                tokio::time::sleep(Duration::from_secs(60)).await;
                return Ok(WorkSchedule::Idle);
            } else {
                self.initialised = true;
            }
        }

        // TODO: timed stats

        // Handle transaction submissions
        if let Some(ref mut tx_recv) = stage.tx_submission_recv {
            while let Ok(transaction) = tx_recv.try_recv() {
                info!("Submitting transaction: {}", transaction.compute_txid());
                if let Err(e) = self.peer_session.submit_transaction(transaction).await {
                    error!("Failed to submit transaction: {:?}", e);
                } else {
                    info!("Transaction submitted successfully");
                }
            }
        }

        let mut units = vec![];

        // if we have not reached the tip
        if !self.tip_reached {
            let new_actions = self.fetch_chain_actions(stage).await?;

            if new_actions.is_empty() {
                // peer returned no headers, so we must be at tip
                // await new block or mempool refresh
                self.tip_reached = true;
            }

            units.extend(new_actions);
        }

        if self.tip_reached {
            // Wait for a new block notification with a 5-second timeout TODO config
            let _ = timeout(
                Duration::from_secs(5),
                self.peer_session.new_block_notification.notified(),
            )
            .await;

            let new_actions = self.fetch_chain_actions(stage).await?;
            units.extend(new_actions);

            if let Some(rpc) = self.peer_rpc.as_mut() {
                // Try fetch mempool (TODO: temporary mempool logic)
                if stage.mempool_enabled {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("Time went backwards")
                        .as_secs();

                    let block_template = rpc
                        .get_block_template(
                            GetBlockTemplateModes::Template,
                            &[
                                GetBlockTemplateRules::SegWit,
                                GetBlockTemplateRules::Taproot,
                            ],
                            &[],
                        )
                        .or_restart()?;

                    let prev_bh = block_template.previous_block_hash;

                    let block_txs = block_template
                        .transactions
                        .into_iter()
                        .map(|x| TransactionWithId {
                            tx_id: x.txid,
                            tx: x.transaction().unwrap(),
                        })
                        .collect::<Vec<_>>();

                    units.push(ChainEvent::MempoolBlocks(
                        MempoolSnapshotInfo {
                            timestamp,
                            tip: prev_bh,
                        },
                        vec![block_txs],
                    ));
                }
            }
        }

        if units.is_empty() {
            Ok(WorkSchedule::Idle)
        } else {
            Ok(WorkSchedule::Unit(units))
        }
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

        if let Some(x) = &self.has_shutdown {
            x.send(()).await.or_panic()?
        };

        Ok(())
    }
}

pub struct Worker {
    peer_session: Peer,
    peer_rpc: Option<RpcClient>,
    cursor: Vec<Point>,
    stats: PullStats,
    initialised: bool,
    tip: Point,
    tip_reached: bool,
    has_shutdown: Option<mpsc::Sender<()>>,
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

    async fn fetch_chain_actions(
        &mut self,
        stage: &mut Stage,
    ) -> Result<Vec<ChainEvent>, WorkerError> {
        let mut units = vec![];

        // fetch new headers using cursor
        let mut headers = self
            .peer_session
            .get_new_headers(self.cursor.iter().map(|x| x.hash).collect())
            .await
            .or_restart()?;

        let Some(first_header) = headers.first().cloned() else {
            return Ok(vec![]);
        };

        // fetch configurable amount of blocks at once
        headers.truncate(10); // TODO: config

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

        // find the height of the first returned header using the cursor
        let intersect_height = cursor_map
            .get(&first_header.prev_blockhash)
            .ok_or_else(|| {
                warn!(
                    "could not find intersect height for {:?}",
                    first_header.prev_blockhash
                );
                WorkerError::Restart
            })?;

        // send a rollback to the intersect if it was behind our tip
        if let Some(cursor_tip_height) = cursor_tip {
            if *intersect_height != cursor_tip_height {
                units.push(ChainEvent::RollBack(Point {
                    height: *intersect_height,
                    hash: first_header.prev_blockhash,
                }))
            }
        }

        // assign heights to the received headers using the intersect height
        let headers = headers
            .into_iter()
            .zip((*intersect_height + 1)..)
            .collect::<Vec<_>>();

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
                stage.intersect,
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

            if height > self.tip.height {
                self.tip = Point {
                    height,
                    hash: header_hash,
                }
            }

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

            units.push(ChainEvent::RollForward(point, header, block_txs, self.tip));
        }

        Ok(units)
    }

    async fn process_action(
        &mut self,
        stage: &mut Stage,
        next: &ChainEvent,
    ) -> Result<(), WorkerError> {
        match next {
            p @ ChainEvent::RollForward(_point, ..) => {
                // info!(?point, "pull roll forward");
                self.send(stage, p.clone()).await.or_panic()?;
                Ok(())
            }
            p @ ChainEvent::RollBack(_point) => {
                // info!(?point, "pull rollback");
                self.send(stage, p.clone()).await.or_panic()?;
                Ok(())
            }
            p @ ChainEvent::MempoolBlocks(_info, ..) => {
                // info!(?info, "pull mempool blocks");
                self.send(stage, p.clone()).await.or_panic()?;
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
                1000_f64 / time_taken.as_secs_f64()
            );
        }
    }
}

#[allow(dead_code)]
/// Estimate tip by downloading headers using only P2P (currently using RPC)
// TODO: improve so we can reuse these fetched headers instead of fetching again
async fn estimate_tip(peer: &mut Peer, after: Point) -> Result<Point, Error> {
    info!("estimating tip (starting from {after:?}");
    let mut greatest = after;

    loop {
        let batch = peer.get_new_headers(vec![greatest.hash]).await?;

        let Some(batch_greatest) = batch.last() else {
            break;
        };

        // there was a rollback
        if batch[0].prev_blockhash != greatest.hash {
            break;
        }

        greatest = Point {
            height: greatest.height + batch.len() as u64,
            hash: batch_greatest.block_hash(),
        }
    }

    Ok(greatest)
}

/// If we give the node the genesis hash and it returns no headers, the node must be syncing.
async fn node_syncing(peer: &mut Peer, genesis: BlockHash) -> Result<bool, Error> {
    Ok(peer.get_new_headers(vec![genesis]).await?.is_empty())
}
