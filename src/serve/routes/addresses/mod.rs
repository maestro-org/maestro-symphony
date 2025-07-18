pub mod all_rune_balances;
pub mod all_rune_utxos;
pub mod runes_by_tx;
pub mod specific_rune_balance;
pub mod specific_rune_utxos;
pub mod tx_count_by_address;
pub mod utxos_by_address;

use axum::{Router, routing::get};

use crate::serve::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/{address}/utxos",
            get(utxos_by_address::addresses_utxos_by_address),
        )
        .route(
            "/{address}/tx_count",
            get(tx_count_by_address::addresses_tx_count_by_address),
        )
        .route(
            "/{address}/runes/utxos",
            get(all_rune_utxos::addresses_all_rune_utxos),
        )
        .route(
            "/{address}/runes/utxos/{rune}",
            get(specific_rune_utxos::addresses_specific_rune_utxos),
        )
        .route(
            "/{address}/runes/balances",
            get(all_rune_balances::addresses_all_rune_balances),
        )
        .route(
            "/{address}/runes/balances/{rune}",
            get(specific_rune_balance::addresses_specific_rune_balance),
        )
        .route(
            "/{address}/runes/txs/{txid}",
            get(runes_by_tx::addresses_runes_by_tx),
        )
}
