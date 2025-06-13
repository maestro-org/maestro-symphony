pub mod all_rune_balances;
pub mod all_rune_utxos;
pub mod specific_rune_balance;
pub mod specific_rune_utxos;

use crate::serve::AppState;
use axum::{Router, routing::get};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{address}/runes/utxos", get(all_rune_utxos::handler))
        .route(
            "/{address}/runes/utxos/{rune}",
            get(specific_rune_utxos::handler),
        )
        .route("/{address}/runes/balances", get(all_rune_balances::handler))
        .route(
            "/{address}/runes/balances/{rune}",
            get(specific_rune_balance::handler),
        )
}
