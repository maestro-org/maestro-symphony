pub mod all_rune_balances;
pub mod all_rune_utxos;
pub mod runes_by_tx;
pub mod specific_rune_balance;
pub mod specific_rune_utxos;
pub mod utxos_by_address;

use axum::{Router, routing::get};

use crate::serve::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{address}/utxos", get(utxos_by_address::handler))
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
        .route("/{address}/runes/txs/{txid}", get(runes_by_tx::handler))
}
