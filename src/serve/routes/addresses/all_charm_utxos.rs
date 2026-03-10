use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::routes::charms::charms_util::decode_utxo_charms;
use crate::serve::types::{CharmUtxo, MempoolParam, ServeResponse};
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::charms::tables::CharmsUtxosByScriptKV;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use std::str::FromStr;

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/charms/utxos",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware (default: false)"),
    ),
    responses(
        (status = 200, description = "All charm UTXOs at address", body = ServeResponse<Vec<CharmUtxo>>),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Not found"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Charm UTXOs by Address
///
/// Returns all UTXOs controlled by the provided address which contain charms, sorted by height.
pub async fn addresses_all_charm_utxos(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

    let address = bitcoin::Address::from_str(&address)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;

    let script_pk = address.assume_checked().script_pubkey().to_bytes();

    let range = CharmsUtxosByScriptKV::encode_range(
        Some(&(script_pk.clone(), u64::MIN)),
        Some(&(script_pk, u64::MAX)),
    );

    let iter = storage.iter_kvs::<CharmsUtxosByScriptKV>(range, false);
    let mut utxos = Vec::new();

    for kv in iter {
        let (key, _) = kv?;

        let utxo = storage.get_expected::<UtxoByTxoRefKV>(&key.txo_ref)?;

        let raw = utxo
            .extended
            .get(&TransactionIndexer::Charms)
            .ok_or_else(|| ServeError::internal("missing expected charms data"))?;

        let charms = decode_utxo_charms(raw)?;

        utxos.push(CharmUtxo {
            tx_hash: Txid::from_byte_array(key.txo_ref.tx_hash).to_string(),
            output_index: key.txo_ref.txo_index,
            height: key.produced_height,
            satoshis: utxo.satoshis.to_string(),
            charms,
        });
    }

    let out = ServeResponse {
        data: utxos,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(out)))
}
