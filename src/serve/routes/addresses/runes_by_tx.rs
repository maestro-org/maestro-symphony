use crate::serve::QueryParams;
use crate::serve::error::ServeError;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::ServeResponse;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::custom::runes::tables::{
    RuneActivityByTxKV, RuneActivityByTxKey, RuneBalanceChange,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use bitcoin::{Address, Txid, hashes::Hash};
use serde::Serialize;
use std::str::FromStr;

#[derive(Serialize)]
pub struct RuneEdict {
    rune_id: String,
    amount: String,
    output: Option<u32>,
    block_height: u64,
}

pub async fn handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
    Path((address_str, txid_str)): Path<(String, String)>,
) -> Result<impl IntoResponse, ServeError> {
    // Parse inputs
    let address = Address::from_str(&address_str)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;
    let txid =
        Txid::from_str(&txid_str).map_err(|_| ServeError::malformed_request("invalid txid"))?;

    let script_pk = address.assume_checked().script_pubkey().to_bytes();

    // Obtain a reader (optionally including mempool data)
    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

    let tx_hash = txid.to_byte_array();

    // Start and end keys to confine the iterator to this transaction only
    let start_key = RuneActivityByTxKey { tx_hash, seq: 0 };
    // u16::MAX is the highest possible sequence for the same tx_hash
    let end_key = RuneActivityByTxKey {
        tx_hash,
        seq: u16::MAX,
    };

    let range = RuneActivityByTxKV::encode_range(Some(&start_key), Some(&end_key));

    let iter = storage.iter_kvs::<RuneActivityByTxKV>(range, false);

    let mut out: Vec<RuneEdict> = Vec::new();

    for kv in iter {
        let (_, change): (_, RuneBalanceChange) = kv?;

        // Only consider balance changes affecting the requested script (address)
        if change.script != script_pk {
            continue;
        }

        // Use recorded output index if provided (None for spends)
        out.push(RuneEdict {
            rune_id: change.rune_id.to_string(),
            amount: change.received.to_string(),
            output: change.output_index,
            block_height: change.rune_id.block,
        });
    }

    let resp = ServeResponse {
        data: out,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(resp)))
}
