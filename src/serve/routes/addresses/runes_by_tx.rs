use crate::serve::error::ServeError;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::{MempoolParam, RuneEdict, ServeResponse};
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
use std::str::FromStr;

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/runes/txs/{txid}",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),
        ("txid" = String, Path, description = "Transaction ID", example="c0345bb5906257a05cdc2d11b6580ce75fdfe8b7ac09b7b2711d435e2ba0a9b3"),

        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<Vec<RuneEdict>>,
            example = json!(EXAMPLE_RESPONSE)
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested entity not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Rune Edicts by Address and Transaction
pub async fn addresses_runes_by_tx(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
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

static EXAMPLE_RESPONSE: &str = r##"{
  "data": [
    {
      "rune_id": "30562:50",
      "amount": "100000000",
      "output": 1,
      "block_height": 30562
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
