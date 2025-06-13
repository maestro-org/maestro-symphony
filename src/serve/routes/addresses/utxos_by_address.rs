use crate::serve::error::ServeError;
use crate::serve::routes::addresses::AppState;
use crate::storage::encdec::Decode;
use crate::storage::table::Table;
use crate::storage::timestamp::Timestamp;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::utxos_by_address::UtxosByAddressKV;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use rocksdb::ReadOptions;
use serde::Serialize;
use std::str::FromStr;

#[derive(Serialize)]
pub struct AddressUtxo {
    tx_hash: String,
    output_index: u32,
    height: u64,
    satoshis: String,
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

    // TODO: query params
    let from_height = u64::MIN;
    let to_height = u64::MAX;

    // kv range for address (scriptpk) rune utxos within height range (inclusive)
    let range = UtxosByAddressKV::encode_range(
        Some(&(script_pk.clone(), from_height)),
        Some(&(script_pk, to_height.saturating_add(1))),
    );

    let latest_ts = Timestamp::from_u64(u64::MAX); // TODO: use ts from storage
    let iter = storage.iter_kvs::<UtxosByAddressKV>(range, latest_ts, false);

    let mut read_opts = ReadOptions::default();
    read_opts.set_timestamp(latest_ts.as_rocksdb_ts());

    let mut utxos = Vec::new();

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

        // Create UTXO entry for response
        utxos.push(AddressUtxo {
            tx_hash: Txid::from_byte_array(key.txo_ref.tx_hash).to_string(),
            output_index: key.txo_ref.txo_index,
            height: key.produced_height,
            satoshis: utxo_val.satoshis.to_string(),
        });
    }

    Ok((StatusCode::OK, Json(utxos)))
}
