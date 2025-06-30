use crate::serve::AppState;
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::types::{MempoolParam, RuneInfo, RuneInfoBatch, RuneTerms, ServeResponse};
use crate::serve::utils::RuneIdentifier;
use crate::sync::stages::index::indexers::custom::runes::tables::{RuneIdByNameKV, RuneInfoByIdKV};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use ordinals::{Rune, SpacedRune};

use std::collections::HashMap;
use std::collections::HashSet;

#[utoipa::path(
    tag = "Runes (Metaprotocol)",
    post,
    path = "/runes/info",
    request_body = Vec<String>,
    params(
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<RuneInfoBatch>,
            // example = json!({})
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested entity not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Info by Rune (Batch)
///
/// Given a list of rune identifiers (name or id), returns a map of identifiers to rune info and a list of identifiers for which information could not be found.
pub async fn runes_rune_info(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Json(rune_ids): Json<Vec<String>>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

    // Deduplicate and clean input upfront
    let unique_ids: HashSet<String> = rune_ids
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    let mut found: HashMap<String, RuneInfo> = HashMap::new();
    let mut missing: Vec<String> = Vec::new();

    // TODO: consider using multi_get to get all the rune info
    for rune_str in unique_ids.into_iter() {
        let rune_id_res = RuneIdentifier::parse(&rune_str);

        let rune_id = match rune_id_res {
            Ok(RuneIdentifier::Id(id)) => id,
            Ok(RuneIdentifier::Name(name_num)) => {
                match storage.get_maybe::<RuneIdByNameKV>(&name_num)? {
                    Some(id) => id,
                    None => {
                        missing.push(rune_str);
                        continue;
                    }
                }
            }
            Err(_) => {
                missing.push(rune_str);
                continue;
            }
        };

        let key_string = rune_id.to_string();

        if found.contains_key(&key_string) {
            continue;
        }

        let rune_info = match storage.get_maybe::<RuneInfoByIdKV>(&rune_id)? {
            Some(info) => info,
            None => {
                missing.push(rune_str);
                continue;
            }
        };

        let rune = Rune(rune_info.name);
        let spaced = SpacedRune::new(rune, rune_info.spacers);

        let info = RuneInfo {
            id: rune_id.to_string(),
            name: rune.to_string(),
            spaced_name: spaced.to_string(),
            symbol: rune_info.symbol.map(|x| char::from_u32(x)).flatten(),
            divisibility: rune_info.divisibility,
            etching_tx: Txid::from_byte_array(rune_info.etching_tx).to_string(),
            etching_height: rune_info.etching_height,
            terms: rune_info.terms.map(|x| RuneTerms {
                amount: x.amount.map(|y| y.to_string()),
                cap: x.cap.map(|y| y.to_string()),
                start_height: x.start_height,
                end_height: x.end_height,
            }),
            premine: rune_info.premine.to_string(),
        };

        found.insert(key_string, info);
    }

    let batch = RuneInfoBatch { found, missing };

    let out = ServeResponse {
        data: batch,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(out)))
}
