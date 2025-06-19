use crate::serve::QueryParams;
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::utils::decimal;
use crate::storage::encdec::Decode;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::runes::tables::{
    RuneInfoByIdKV, RuneUtxosByScriptKV, UtxoRunes,
};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use itertools::Itertools;
use ordinals::{Rune, RuneId, SpacedRune};
use serde::Serialize;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Serialize)]
pub struct RuneAndQuantity {
    id: String,
    name: String,
    spaced_name: String,
    quantity: String,
}

pub async fn handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let storage = if params.mempool.unwrap_or(false) {
        state.start_reader_mempool().await?
    } else {
        state.start_reader_confirmed().await?
    };

    let address = bitcoin::Address::from_str(&address)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;

    // TODO: enforce network?
    let script_pk = address.assume_checked().script_pubkey().to_bytes();

    // kv range for address (scriptpk) rune utxos
    let range = RuneUtxosByScriptKV::encode_range(
        Some(&(script_pk.clone(), u64::MIN)),
        Some(&(script_pk, u64::MAX)),
    );

    let iter = storage.iter_kvs::<RuneUtxosByScriptKV>(range, false);

    let mut balances: HashMap<RuneId, u128> = HashMap::new();

    // TODO: batch gets
    for kv in iter {
        let (key, _) = kv?;

        let utxo = storage.get_expected::<UtxoByTxoRefKV>(&key.txo_ref)?;

        // Get runes data from UTXO
        let utxo_runes_raw = utxo
            .extended
            .get(&TransactionIndexer::Runes)
            .ok_or_else(|| ServeError::internal("missing expected data"))?;

        // Decode runes data
        let utxo_runes = UtxoRunes::decode_all(utxo_runes_raw)?;

        for (rune_id, raw_quantity) in utxo_runes {
            *balances.entry(rune_id).or_default() += raw_quantity
        }
    }

    let mut processed_balances = vec![];

    for (rune_id, raw_quantity) in balances.into_iter().sorted_by_key(|(rid, _)| *rid) {
        let rune_info = storage.get_expected::<RuneInfoByIdKV>(&rune_id)?;

        let rune = Rune(rune_info.name);
        let spaced = SpacedRune::new(rune, rune_info.spacers);

        processed_balances.push(RuneAndQuantity {
            id: rune_id.to_string(),
            name: rune.to_string(),
            spaced_name: spaced.to_string(),
            quantity: decimal(raw_quantity, rune_info.divisibility),
        })
    }

    Ok((StatusCode::OK, Json(processed_balances)))
}
