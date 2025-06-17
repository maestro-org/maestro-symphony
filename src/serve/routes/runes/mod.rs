pub mod rune_info;

use crate::serve::AppState;
use axum::{Router, routing::get};

pub fn router() -> Router<AppState> {
    Router::new().route("/{rune}", get(rune_info::handler))
}
