pub mod rune_info_batch;

use crate::serve::AppState;
use axum::{Router, routing::post};

pub fn router() -> Router<AppState> {
    Router::new().route("/info", post(rune_info_batch::runes_rune_info_batch))
}
