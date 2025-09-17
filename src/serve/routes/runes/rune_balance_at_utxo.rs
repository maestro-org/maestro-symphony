use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::types::{MempoolParam, ServeResponse};
use crate::serve::utils::RuneIdentifier;
use crate::storage::encdec::Decode;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::runes::tables::{RuneIdByNameKV, UtxoRunes};
use crate::sync::stages::index::indexers::types::TxoRef;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use bitcoin::{Txid, hashes::Hash};
use std::str::FromStr;

use crate::serve::AppState;

#[utoipa::path(
    tag = "Runes (Metaprotocol)",
    get,
    path = "/runes/{rune}/utxos/{utxo}/balance",
    params(
        ("utxo" = String, Path, description = "UTXO reference in format txid:index", example="c0345bb5906257a05cdc2d11b6580ce75fdfe8b7ac09b7b2711d435e2ba0a9b3:1"),
        ("rune" = String, Path, description = "Rune ID or name (spaced or unspaced)", example="65103:2", example="BITCOIN•PIZZAS"),
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<String>,
            example = json!(serde_json::Value::from_str(EXAMPLE_RESPONSE).unwrap())
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested rune or UTXO not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Rune Balance by UTXO and Rune
///
/// Returns the amount of the specified rune contained in the provided UTXO. If the rune is not
/// present in the UTXO, `0` is returned.
pub async fn rune_balance_at_utxo(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Path((rune, utxo_str)): Path<(String, String)>,
) -> Result<impl IntoResponse, ServeError> {
    // Parse UTXO string (expected format "<txid>:<vout>")
    let mut split = utxo_str.split(':');
    let txid_str = split
        .next()
        .ok_or_else(|| ServeError::malformed_request("invalid utxo reference"))?;
    let vout_str = split
        .next()
        .ok_or_else(|| ServeError::malformed_request("invalid utxo reference"))?;

    if split.next().is_some() {
        return Err(ServeError::malformed_request("invalid utxo reference"));
    }

    let txid = Txid::from_str(txid_str)
        .map_err(|_| ServeError::malformed_request("invalid txid in utxo reference"))?;
    let vout: u32 = vout_str
        .parse()
        .map_err(|_| ServeError::malformed_request("invalid output index in utxo reference"))?;

    let txo_ref = TxoRef {
        tx_hash: txid.to_byte_array(),
        txo_index: vout,
    };

    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

    // Resolve rune identifier -> RuneId
    let specified_rune = match RuneIdentifier::parse(&rune)? {
        RuneIdentifier::Id(id) => id,
        RuneIdentifier::Name(n) => storage
            .get_maybe::<RuneIdByNameKV>(&n)?
            .ok_or_else(|| ServeError::NotFound)?,
    };

    // Fetch the UTXO from storage
    let utxo = storage
        .get_maybe::<UtxoByTxoRefKV>(&txo_ref)?
        .ok_or_else(|| ServeError::NotFound)?;

    // Attempt to extract runes extended data; if none present, balance is 0
    let balance: u128 = if let Some(raw) = utxo.extended.get(&TransactionIndexer::Runes) {
        let utxo_runes = UtxoRunes::decode_all(raw)?;
        utxo_runes
            .into_iter()
            .find(|(id, _)| *id == specified_rune)
            .map(|(_, qty)| qty)
            .unwrap_or(0)
    } else {
        0
    };

    let resp = ServeResponse {
        data: balance.to_string(),
        indexer_info,
    };

    Ok((StatusCode::OK, Json(resp)))
}

static EXAMPLE_RESPONSE: &str = r##"{
  "data": "100000000",
  "indexer_info": {
    "chain_tip": {
      "block_hash": "00000000000000108a4cd9755381003a01bea7998ca2d770fe09b576753ac7ef",
      "block_height": 31633
    },
    "mempool_timestamp": null,
    "estimated_blocks": []
  }
}"##;
