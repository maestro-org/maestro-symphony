use crate::serve::AppState;
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::types::{MempoolParam, RuneInfo, RuneTerms, ServeResponse};
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
    request_body(content = Vec<String>, example = json!(["BESTINSLOTXYZ", "ABCDEF"])),
    params(
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<HashMap<String, Option<RuneInfo>>>,
            example = json!(EXAMPLE_RESPONSE)
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested entity not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Info by Rune (Batch)
///
/// Given a list of rune identifiers (name or id), returns a map of identifiers to rune info (or null if no info found).
pub async fn runes_rune_info_batch(
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

    let mut infos: HashMap<String, Option<RuneInfo>> = HashMap::new();

    // TODO: consider using multi_get to get all the rune info
    for rune_str in unique_ids.into_iter() {
        let rune_id_res = RuneIdentifier::parse(&rune_str);

        let rune_id = match rune_id_res {
            Ok(RuneIdentifier::Id(id)) => id,
            Ok(RuneIdentifier::Name(name_num)) => {
                match storage.get_maybe::<RuneIdByNameKV>(&name_num)? {
                    Some(id) => id,
                    None => {
                        infos.insert(rune_str, None);
                        continue;
                    }
                }
            }
            Err(_) => {
                infos.insert(rune_str, None);
                continue;
            }
        };

        let key_string = rune_id.to_string();

        if infos.contains_key(&key_string) {
            continue;
        }

        let rune_info = match storage.get_maybe::<RuneInfoByIdKV>(&rune_id)? {
            Some(info) => info,
            None => {
                infos.insert(rune_str, None);
                continue;
            }
        };

        let rune = Rune(rune_info.name);
        let spaced = SpacedRune::new(rune, rune_info.spacers);

        let info = RuneInfo {
            id: rune_id.to_string(),
            name: rune.to_string(),
            spaced_name: spaced.to_string(),
            symbol: rune_info.symbol.and_then(char::from_u32),
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

        infos.insert(key_string, Some(info));
    }

    let out = ServeResponse {
        data: infos,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(out)))
}

static EXAMPLE_RESPONSE: &str = r##"{
  "data": {
    "30562:50": {
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
    },
    "ABCDEF": null
  },
  "indexer_info": {
    "chain_tip": {
      "block_hash": "00000000000000108a4cd9755381003a01bea7998ca2d770fe09b576753ac7ef",
      "block_height": 31633
    },
    "mempool_timestamp": null,
    "estimated_blocks": []
  }
}"##;
