use crate::serve::AppState;
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::types::{CharmAndValue, MempoolParam, ServeResponse};
use crate::serve::utils::parse_charm_path;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::types::TxoRef;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use bitcoin::{Txid, hashes::Hash};
use std::str::FromStr;

use super::charms_util::decode_utxo_charms;

#[utoipa::path(
    tag = "Charms (Metaprotocol)",
    get,
    path = "/charms/{charm}/utxos/{utxo}/value",
    params(
        ("utxo" = String, Path, description = "UTXO reference in format txid:index", example="c0345bb5906257a05cdc2d11b6580ce75fdfe8b7ac09b7b2711d435e2ba0a9b3:1"),
        ("charm" = String, Path, description = "Charm app in URI-safe form (dashes instead of slashes)", example="t-0000000000000000000000000000000000000000000000000000000000000001-0000000000000000000000000000000000000000000000000000000000000002"),
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (status = 200, description = "Charm value at UTXO", body = ServeResponse<CharmAndValue>),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Charm or UTXO not found"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Charm Value at UTXO
///
/// Returns the value of the specified charm at the provided UTXO. For fungible tokens (tag `t`),
/// the value is a u64 amount. For other charms, it can be any JSON value.
pub async fn charm_value_at_utxo(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Path((charm, utxo_str)): Path<(String, String)>,
) -> Result<impl IntoResponse, ServeError> {
    let canonical_app = parse_charm_path(&charm);

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

    let utxo = storage
        .get_maybe::<UtxoByTxoRefKV>(&txo_ref)?
        .ok_or(ServeError::NotFound)?;

    let raw = utxo
        .extended
        .get(&TransactionIndexer::Charms)
        .ok_or(ServeError::NotFound)?;

    let charms = decode_utxo_charms(raw)?;

    let charm_value = charms
        .into_iter()
        .find(|c| c.app == canonical_app)
        .ok_or(ServeError::NotFound)?;

    let resp = ServeResponse {
        data: charm_value,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(resp)))
}
