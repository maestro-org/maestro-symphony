pub mod charm_latest;
pub mod charm_value_at_utxo;
pub(crate) mod charms_util;

use crate::serve::AppState;
use axum::{Router, routing::get};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{charm}/latest", get(charm_latest::charm_latest))
        .route(
            "/{charm}/utxos/{utxo}/value",
            get(charm_value_at_utxo::charm_value_at_utxo),
        )
}
