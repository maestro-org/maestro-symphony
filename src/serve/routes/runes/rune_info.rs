use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::types::ServeResponse;
use crate::serve::utils::RuneIdentifier;
use crate::serve::{AppState, QueryParams};
use crate::sync::stages::index::indexers::custom::runes::tables::{RuneIdByNameKV, RuneInfoByIdKV};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use ordinals::{Rune, SpacedRune};
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Serialize)]
pub struct RuneInfo {
    id: String,
    name: String,
    spaced_name: String,
    symbol: Option<char>,
    divisibility: u8,
    etching_tx: String,
    etching_height: u64,
    terms: Option<RuneTerms>,
    premine: String,
}

#[derive(Serialize)]
pub struct RuneTerms {
    amount: Option<String>,
    cap: Option<String>,
    start_height: Option<u64>,
    end_height: Option<u64>,
}

#[derive(Serialize)]
pub struct RuneInfoBatch {
    found: HashMap<String, RuneInfo>,
    missing: Vec<String>,
}

pub async fn handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
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
