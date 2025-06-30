pub mod rune_info;

use crate::serve::AppState;
use axum::{Router, routing::post};

pub fn router() -> Router<AppState> {
    Router::new().route("/info", post(rune_info::runes_rune_info))
}
