use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::{
    RuneBalanceWithInfo, RuneBalancesParam, RuneInfo, RuneTerms, ServeResponse,
};
use crate::storage::encdec::Decode;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::runes::tables::{
    RuneInfoByIdKV, RuneUtxosByScriptKV, UtxoRunes,
};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use itertools::Itertools;
use ordinals::{Rune, RuneId, SpacedRune};
use std::collections::HashMap;
use std::str::FromStr;

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/runes/balances",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),

        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware (default: false)"),
        ("include_info" = inline(Option<bool>), Query, description = "Include rune info for each balance (default: false)"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<Vec<RuneBalanceWithInfo>>,
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
    Query(params): Query<RuneBalancesParam>,
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

    let mut processed_balances: Vec<RuneBalanceWithInfo> = vec![];

    for (rune_id, amount) in balances.into_iter().sorted_by_key(|(rid, _)| *rid) {
        let rune_info = if params.include_info {
            let info = storage.get_expected::<RuneInfoByIdKV>(&rune_id)?;
            let rune = Rune(info.name);
            let spaced = SpacedRune::new(rune, info.spacers);

            Some(RuneInfo {
                id: rune_id.to_string(),
                name: rune.to_string(),
                spaced_name: spaced.to_string(),
                symbol: info.symbol.and_then(char::from_u32),
                divisibility: info.divisibility,
                etching_tx: Txid::from_byte_array(info.etching_tx).to_string(),
                etching_height: info.etching_height,
                terms: info.terms.map(|x| RuneTerms {
                    amount: x.amount.map(|y| y.to_string()),
                    cap: x.cap.map(|y| y.to_string()),
                    start_height: x.start_height,
                    end_height: x.end_height,
                }),
                premine: info.premine.to_string(),
            })
        } else {
            None
        };

        processed_balances.push(RuneBalanceWithInfo {
            id: rune_id.to_string(),
            amount: amount.to_string(),
            info: rune_info,
        });
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
      "amount": "100000000",
      "info": {
        "id": "30562:50",
        "name": "BESTINSLOTXYZ",
        "spaced_name": "BESTINSLOT•XYZ",
        "symbol": "ʃ",
        "divisibility": 8,
        "etching_tx": "63937d48e35d15a7c5530469210c202104cc94a945cc848554f336b3f4f24121",
        "etching_height": 30562,
        "terms": {
          "amount": "100000000",
          "cap": "3402823669209384634633746074316",
          "start_height": null,
          "end_height": null
        },
        "premine": "100000000"
      }
    },
    {
      "id": "63523:1",
      "amount": "990000",
      "info": {
        "id": "63523:1",
        "name": "JFMJFMJFMHHHAAA",
        "spaced_name": "JFMJFMJFMHHHAAA",
        "symbol": "J",
        "divisibility": 2,
        "etching_tx": "3bcc9e8f8eaf120ea5af65a378925b703f6fb1960435629eef5cb5900c19bec9",
        "etching_height": 63523,
        "terms": {
          "amount": "100000",
          "cap": "100",
          "start_height": 63515,
          "end_height": null
        },
        "premine": "1000000"
      }
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
