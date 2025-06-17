use crate::serve::error::ServeError;
use crate::serve::routes::addresses::AppState;
use crate::serve::utils::decimal;
use crate::storage::encdec::Decode;
use crate::storage::table::Table;
use crate::storage::timestamp::Timestamp;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::runes::tables::{
    RuneInfoByIdKV, RuneUtxosByScriptKV, UtxoRunes,
};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use itertools::Itertools;
use ordinals::{Rune, RuneId, SpacedRune};
use rocksdb::ReadOptions;
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
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let storage = state.read().await;
    let cf = storage.cf_handle(); // TODO: hide

    let address = bitcoin::Address::from_str(&address)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;

    // TODO: enforce network?
    let script_pk = address.assume_checked().script_pubkey().to_bytes();

    // kv range for address (scriptpk) rune utxos
    let range = RuneUtxosByScriptKV::encode_range(
        Some(&(script_pk.clone(), u64::MIN)),
        Some(&(script_pk, u64::MAX)),
    );

    let latest_ts = Timestamp::from_u64(u64::MAX); // TODO: use ts from storage
    let iter = storage.iter_kvs::<RuneUtxosByScriptKV>(range, latest_ts, false);

    let mut read_opts = ReadOptions::default();
    read_opts.set_timestamp(latest_ts.as_rocksdb_ts());

    let mut balances: HashMap<RuneId, u128> = HashMap::new();

    // TODO: batch gets
    for kv in iter {
        let (key, _) = kv?;

        // Get UTXO data from the database
        let res = storage
            .db
            .get_cf_opt(&cf, UtxoByTxoRefKV::encode_key(&key.txo_ref), &read_opts)?
            .ok_or_else(|| ServeError::internal("missing expected data"))?;

        // Decode UTXO value
        let utxo_val = <UtxoByTxoRefKV as Table>::Value::decode_all(&res)?;

        // Get runes data from UTXO
        let utxo_runes_raw = utxo_val
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
        let raw_rune_info = storage
            .db
            .get_cf_opt(&cf, RuneInfoByIdKV::encode_key(&rune_id), &read_opts)?
            .ok_or_else(|| ServeError::internal("missing expected data"))?;

        // Decode UTXO value
        let utxo_val = <RuneInfoByIdKV as Table>::Value::decode_all(&raw_rune_info)?;

        let rune = Rune(utxo_val.name);
        let spaced = SpacedRune::new(rune, utxo_val.spacers);

        processed_balances.push(RuneAndQuantity {
            id: rune_id.to_string(),
            name: rune.to_string(),
            spaced_name: spaced.to_string(),
            quantity: decimal(raw_quantity, utxo_val.divisibility),
        })
    }

    Ok((StatusCode::OK, Json(processed_balances)))
}
