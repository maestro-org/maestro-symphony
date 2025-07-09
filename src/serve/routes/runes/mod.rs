pub mod rune_balance_at_utxo;
pub mod rune_info_batch;

use crate::serve::AppState;
use axum::{
    Router,
    routing::{get, post},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/info", post(rune_info_batch::runes_rune_info_batch))
        .route(
            "/{rune}/utxos/{utxo}/balance",
            get(rune_balance_at_utxo::rune_balance_at_utxo),
        )
}
