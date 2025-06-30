use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::{MempoolParam, RuneAndAmount, RuneUtxo, ServeResponse};
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
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use itertools::Itertools;
use std::str::FromStr;

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/runes/utxos/{rune}",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),
        ("rune" = String, Path, description = "Rune ID or name (spaced or unspaced)", example="65103:2", example="BITCOIN•PIZZAS"),
        
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = ServeResponse<Vec<RuneUtxo>>,
            // example = json!({})
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Requested entity not found on-chain"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Rune UTxOs by Address and Rune
///
/// Returns UTxOs controlled by the provided address which contain the provided rune, sorted by height.
pub async fn addresses_specific_rune_utxos(
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

    // TODO: query params
    let from_height = u64::MIN;
    let to_height = u64::MAX;

    // kv range for address (scriptpk) rune utxos within height range (inclusive)
    let range = RuneUtxosByScriptKV::encode_range(
        Some(&(script_pk.clone(), from_height)),
        Some(&(script_pk, to_height.saturating_add(1))),
    );

    let iter = storage.iter_kvs::<RuneUtxosByScriptKV>(range, false);

    let mut utxos = Vec::new();

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

        // skip utxo if it doesn't contain specified rune
        if !utxo_runes
            .iter()
            .map(|(rid, _)| rid)
            .contains(&specified_rune)
        {
            continue;
        }

        let mut processed_runes = Vec::with_capacity(utxo_runes.len());

        for (rune_id, amount) in utxo_runes {
            processed_runes.push(RuneAndAmount {
                id: rune_id.to_string(),
                amount: amount.to_string(),
            })
        }

        // Create UTXO entry for response
        utxos.push(RuneUtxo {
            tx_hash: Txid::from_byte_array(key.txo_ref.tx_hash).to_string(),
            output_index: key.txo_ref.txo_index,
            height: key.produced_height,
            satoshis: utxo.satoshis.to_string(),
            runes: processed_runes,
        });
    }

    let out = ServeResponse {
        data: utxos,
        indexer_info,
    };

    Ok((StatusCode::OK, Json(out)))
}
