use std::time::Duration;

use bitcoincore_rpc::Auth;
use tokio::sync::{mpsc, oneshot::Receiver};

use crate::{error::Error, storage::kv_store::StorageHandler, sync::stages::index};

use super::{Config, stages::pull};

const DEFAULT_SYNC_STAGE_QUEUE_SIZE: usize = 20;
const DEFAULT_SYNC_STAGE_TIMEOUT_SECS: u64 = 600;

// TODO: Config
fn gasket_policy(stage_timeout: u64) -> gasket::runtime::Policy {
    let default_retries = gasket::retries::Policy {
        max_retries: 20,
        backoff_unit: Duration::from_secs(1),
        backoff_factor: 2,
        max_backoff: Duration::from_secs(60),
        ..Default::default()
    };

    gasket::runtime::Policy {
        tick_timeout: std::time::Duration::from_secs(stage_timeout).into(),
        bootstrap_retry: default_retries.clone(),
        work_retry: default_retries.clone(),
        teardown_retry: default_retries,
    }
}

pub fn pipeline(
    config: Config,
    db: StorageHandler,
    shutdown_signals: Option<(Receiver<()>, mpsc::Sender<()>)>,
) -> Result<gasket::daemon::Daemon, Error> {
    // * use db to find cursor / rollback buffer, pass to both stages where relevant

    // create Index stage for processing blocks and storing data
    let mut index = index::worker::stage::Stage::new(config.clone(), db.clone())?;

    let rpc_auth = Auth::UserPass(config.node.rpc_user, config.node.rpc_pass);

    // Initialize transaction submission channel
    let (tx_sender, tx_receiver) = mpsc::unbounded_channel();

    // Store the sender in the global static for the HTTP endpoint to use
    crate::serve::set_tx_submission_sender(tx_sender);

    // create Pull stage for pulling blocks/mempool from node
    let mut pull = pull::Stage::new(
        config.node.p2p_address,
        config.node.rpc_address,
        rpc_auth,
        config.network,
        config.mempool,
        config.intersect,
        db,
        shutdown_signals,
        Some(tx_receiver),
    );

    // // create Health stage for exposing health info
    // let mut health = health::Stage::new();

    // connect stages

    let queue_size = config
        .stage_queue_size
        .unwrap_or(DEFAULT_SYNC_STAGE_QUEUE_SIZE);
    let stage_timeout = config
        .stage_timeout_secs
        .unwrap_or(DEFAULT_SYNC_STAGE_TIMEOUT_SECS);

    let (pull_to_index, index_from_pull) = gasket::messaging::tokio::mpsc_channel(queue_size);
    pull.downstream.connect(pull_to_index);
    index.upstream.connect(index_from_pull);

    // let (index_to_health, index_from_health) = gasket::messaging::tokio::mpsc_channel(queue_size);
    // index.health_downstream.connect(index_to_health);
    // health.index_upstream.connect(index_from_health);

    // spawn stages

    let policy = gasket_policy(stage_timeout);

    let pull = gasket::runtime::spawn_stage(pull, policy.clone());
    let index = gasket::runtime::spawn_stage(index, policy.clone());
    // let health = gasket::runtime::spawn_stage(health, policy);

    Ok(gasket::daemon::Daemon::new(vec![pull, index]))
}
