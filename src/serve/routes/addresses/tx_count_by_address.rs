use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::{MempoolParam, ServeResponse};
use crate::sync::stages::index::indexers::custom::tx_count_by_address::TxCountByAddressKV;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use std::str::FromStr;

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/tx_count",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),

        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<u64>,
            example = json!(EXAMPLE_RESPONSE)
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested entity not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Transaction Count by Address
///
/// Returns the number of transactions in which the address controlled an input or output
pub async fn addresses_tx_count_by_address(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

    let address = bitcoin::Address::from_str(&address)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;

    // TODO: enforce network?
    let script_pk = address.assume_checked().script_pubkey().to_bytes();

    let count = storage
        .get_maybe::<TxCountByAddressKV>(&script_pk)?
        .unwrap_or(0);

    let out = ServeResponse {
        data: count,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(out)))
}

static EXAMPLE_RESPONSE: &str = r##"{
  "data": 21086,
  "indexer_info": {
    "chain_tip": {
      "block_hash": "0000000004af5483f4e54ebf8f1d728b003d19ebc184761b82c50b8e86ec2a0a",
      "block_height": 91625
    },
    "mempool_timestamp": null,
    "estimated_blocks": []
  }
}"##;
