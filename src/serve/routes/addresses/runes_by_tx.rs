use crate::serve::QueryParams;
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::ServeResponse;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::hash_by_height::HashByHeightKV;
use crate::sync::stages::index::indexers::custom::runes::tables::{
    RuneActivityByTxKV, RuneActivityByTxKey, RuneBalanceChange, RuneInfoByIdKV,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use bitcoin::{Address, BlockHash, Txid, hashes::Hash};
use ordinals::Rune;
use serde::Serialize;
use std::str::FromStr;

#[derive(Serialize)]
pub struct RuneEdict {
    rune_id: String,
    amount: String,
    output: u32,
    tx_id: String,
    block_height: u64,
    tx_index: u32,
    divisibility: u8,
    name: String,
    symbol: Option<char>,
    block_hash: String,
    premine: String,
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

        let rune_info = storage.get_expected::<RuneInfoByIdKV>(&change.rune_id)?;

        let block_hash_bytes = storage.get_expected::<HashByHeightKV>(&change.rune_id.block)?;
        let block_hash = BlockHash::from_byte_array(block_hash_bytes);

        let rune = Rune(rune_info.name);

        // Use recorded output index if provided (None for spends)
        out.push(RuneEdict {
            rune_id: change.rune_id.to_string(),
            amount: change.received.to_string(),
            output: change.output_index.unwrap_or(0),
            tx_id: txid.to_string(),
            block_height: change.rune_id.block,
            tx_index: change.rune_id.tx,
            divisibility: rune_info.divisibility,
            name: rune.to_string(),
            symbol: rune_info.symbol.and_then(|s| char::from_u32(s)),
            block_hash: block_hash.to_string(),
            premine: rune_info.premine.to_string(),
        });
    }

    let resp = ServeResponse {
        data: out,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(resp)))
}
