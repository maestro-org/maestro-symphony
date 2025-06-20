use crate::serve::QueryParams;
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::utils::{RuneIdentifier, decimal};
use crate::storage::encdec::Decode;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::runes::tables::{
    RuneIdByNameKV, RuneInfoByIdKV, RuneUtxosByScriptKV, UtxoRunes,
};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use itertools::Itertools;
use ordinals::{Rune, SpacedRune};
use serde::Serialize;
use std::str::FromStr;

#[derive(Serialize)]
pub struct RuneUtxo {
    tx_hash: String,
    output_index: u32,
    height: u64,
    satoshis: String,
    runes: Vec<RuneAndQuantity>,
}

#[derive(Serialize)]
pub struct RuneAndQuantity {
    id: String,
    name: String,
    spaced_name: String,
    quantity: String,
}

pub async fn handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
    Path((address, rune)): Path<(String, String)>,
) -> Result<impl IntoResponse, ServeError> {
    let storage = state.start_reader(params.mempool).await?;

    let address = bitcoin::Address::from_str(&address)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;

    let specified_rune = match RuneIdentifier::parse(rune)? {
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

        for (rune_id, raw_quantity) in utxo_runes {
            let rune_info = storage.get_expected::<RuneInfoByIdKV>(&rune_id)?;

            let rune = Rune(rune_info.name);
            let spaced = SpacedRune::new(rune, rune_info.spacers);

            processed_runes.push(RuneAndQuantity {
                id: rune_id.to_string(),
                name: rune.to_string(),
                spaced_name: spaced.to_string(),
                quantity: decimal(raw_quantity, rune_info.divisibility),
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

    Ok((StatusCode::OK, Json(utxos)))
}
