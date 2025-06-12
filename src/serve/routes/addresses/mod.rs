pub mod rune_utxos;

use crate::storage::kv_store::StorageHandler;
use axum::{Router, routing::get};
use std::sync::Arc;
use tokio::sync::RwLock;

type AppState = Arc<RwLock<StorageHandler>>;

pub fn router() -> Router<AppState> {
    Router::new().route("/{address}/runes/utxos", get(rune_utxos::handler))
}
