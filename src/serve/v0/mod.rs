//! Hosted-Maestro (`/v0`) compatibility surface.
//!
//! Serves the endpoints the Lace wallet consumed from the hosted Maestro
//! Bitcoin API (`xbt-*.gomaestro-api.org`) with byte-compatible payload
//! shapes, so pointing MAESTRO_URL_* at a Symphony deployment requires no
//! wallet change. Address data comes from the local index; the `rpc/*`
//! endpoints bridge to the Bitcoin Core node Symphony syncs from.

use axum::Router;
use axum::routing::{get, post};

use crate::serve::AppState;

pub mod addresses;
pub mod node_rpc;
pub mod rpc;
pub mod types;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/addresses/{address}/txs", get(addresses::txs))
        .route("/addresses/{address}/utxos", get(addresses::utxos))
        .route(
            "/mempool/addresses/{address}/utxos",
            get(addresses::mempool_utxos),
        )
        .route("/rpc/general/info", get(rpc::general_info))
        .route("/rpc/transaction/submit", post(rpc::submit))
        .route("/rpc/transaction/{txid}", get(rpc::transaction))
        .route(
            "/rpc/transaction/estimatefee/{blocks}",
            get(rpc::estimate_fee),
        )
}
