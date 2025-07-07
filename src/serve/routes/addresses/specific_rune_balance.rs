use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::{MempoolParam, ServeResponse};
use crate::serve::utils::RuneIdentifier;
use crate::storage::encdec::Decode;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::runes::tables::{
    RuneIdByNameKV, RuneUtxosByScriptKV, UtxoRunes,
};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use std::str::FromStr;

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/runes/balances/{rune}",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),
        ("rune" = String, Path, description = "Rune ID or name (spaced or unspaced)", example="65103:2", example="BITCOIN•PIZZAS"),

        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<String>,
            example = json!(EXAMPLE_RESPONSE)
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested entity not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Rune Balance by Address and Rune
///
/// Returns the total amount of the provided rune kind held in UTxOs controlled by the provided address.
pub async fn addresses_specific_rune_balance(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Path((address, rune)): Path<(String, String)>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

    let address = bitcoin::Address::from_str(&address)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;

    let specified_rune = match RuneIdentifier::parse(&rune)? {
        RuneIdentifier::Id(x) => x,
        RuneIdentifier::Name(n) => storage
            .get_maybe::<RuneIdByNameKV>(&n)?
            .ok_or_else(|| ServeError::NotFound)?,
    };

    // TODO: enforce network?
    let script_pk = address.assume_checked().script_pubkey().to_bytes();

    // kv range for address (scriptpk) rune utxos
    let range = RuneUtxosByScriptKV::encode_range(
        Some(&(script_pk.clone(), u64::MIN)),
        Some(&(script_pk, u64::MAX)),
    );

    let iter = storage.iter_kvs::<RuneUtxosByScriptKV>(range, false);

    let mut balance: u128 = 0;

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

        for (rune_id, quantity) in utxo_runes {
            if rune_id == specified_rune {
                balance += quantity;
                break;
            }
        }
    }

    let out = ServeResponse {
        data: balance.to_string(),
        indexer_info,
    };

    Ok((StatusCode::OK, Json(out)))
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
