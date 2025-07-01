use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::{MempoolParam, RuneAndAmount, ServeResponse};
use crate::storage::encdec::Decode;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::runes::tables::{RuneUtxosByScriptKV, UtxoRunes};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use itertools::Itertools;
use ordinals::RuneId;
use std::collections::HashMap;
use std::str::FromStr;

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/runes/balances",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),
        
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware (default: false)"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<Vec<RuneAndAmount>>,
            example = json!(EXAMPLE_RESPONSE)
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested entity not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Rune Balances by Address
///
/// Returns total rune balances held in UTxOs controlled by the provided address, sorted by rune ID.
pub async fn addresses_all_rune_balances(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

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

    for (rune_id, amount) in balances.into_iter().sorted_by_key(|(rid, _)| *rid) {
        processed_balances.push(RuneAndAmount {
            id: rune_id.to_string(),
            amount: amount.to_string(),
        })
    }

    let out = ServeResponse {
        data: processed_balances,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(out)))
}

static EXAMPLE_RESPONSE: &str = r##"{
  "data": [
    {
      "id": "30562:50",
      "amount": "100000000"
    },
    {
      "id": "65103:2",
      "amount": "300000"
    }
  ],
  "indexer_info": {
    "chain_tip": {
      "block_hash": "00000000000000108a4cd9755381003a01bea7998ca2d770fe09b576753ac7ef",
      "block_height": 31633
    },
    "mempool_timestamp": null,
    "estimated_blocks": []
  }
}"##;
