use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::{AddressUtxo, MempoolParam, ServeResponse};
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::utxos_by_address::UtxosByAddressKV;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use std::str::FromStr;

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/utxos",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),
        
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<Vec<AddressUtxo>>,
            // example = json!({})
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested entity not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// UTxOs by Address
///
/// Returns UTxOs controlled by the provided address, sorted by height.
pub async fn addresses_utxos_by_address(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

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

    let iter = storage.iter_kvs::<UtxosByAddressKV>(range, false);

    let mut utxos = Vec::new();

    // TODO: batch gets
    for kv in iter {
        let (key, _) = kv?;

        let utxo = storage.get_expected::<UtxoByTxoRefKV>(&key.txo_ref)?;

        // Create UTXO entry for response
        utxos.push(AddressUtxo {
            tx_hash: Txid::from_byte_array(key.txo_ref.tx_hash).to_string(),
            output_index: key.txo_ref.txo_index,
            height: key.produced_height,
            satoshis: utxo.satoshis.to_string(),
        });
    }

    let out = ServeResponse {
        data: utxos,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(out)))
}
