use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::utils::{RuneIdentifier, decimal};
use crate::serve::{AppState, QueryParams};
use crate::sync::stages::index::indexers::custom::runes::tables::{RuneIdByNameKV, RuneInfoByIdKV};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use ordinals::{Rune, SpacedRune};
use serde::Serialize;

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

pub async fn handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
    Path(rune): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let storage = state.start_reader(params.mempool).await?;

    let rune_id = match RuneIdentifier::parse(rune)? {
        RuneIdentifier::Id(x) => x,
        RuneIdentifier::Name(n) => storage
            .get_maybe::<RuneIdByNameKV>(&n)?
            .ok_or_else(|| ServeError::NotFound)?,
    };

    let rune_info = storage
        .get_maybe::<RuneInfoByIdKV>(&rune_id)?
        .ok_or_else(|| ServeError::NotFound)?;

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
            amount: x.amount.map(|y| decimal(y, rune_info.divisibility)),
            cap: x.cap.map(|y| decimal(y, rune_info.divisibility)),
            start_height: x.start_height,
            end_height: x.end_height,
        }),
        premine: decimal(rune_info.premine, rune_info.divisibility),
    };

    Ok((StatusCode::OK, Json(info)))
}
