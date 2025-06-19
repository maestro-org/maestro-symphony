use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::AppState;
use crate::serve::utils::decimal;
use crate::storage::encdec::Decode;
use crate::storage::table::Table;
use crate::storage::timestamp::Timestamp;
use crate::sync::stages::index::indexers::core::hash_by_height::HashByHeightKV;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::runes::tables::{RuneInfoByIdKV, UtxoRunes};
use crate::sync::stages::index::indexers::types::TxoRef;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use bitcoin::{Address, BlockHash, Txid, hashes::Hash};
use ordinals::Rune;
use serde::Serialize;
use std::str::FromStr;

#[derive(Serialize)]
pub struct RuneEdict {
    rune_id: String,
    amount: String,
    output: u32,
    tx_id: String,
    block_height: u64,
    tx_index: u32,
    divisibility: u8,
    name: String,
    symbol: Option<char>,
    block_hash: String,
    premine: String,
}

pub async fn handler(
    State(state): State<AppState>,
    Path((address_str, txid_str)): Path<(String, String)>,
) -> Result<impl IntoResponse, ServeError> {
    // Parse inputs
    let address = Address::from_str(&address_str)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;
    let txid =
        Txid::from_str(&txid_str).map_err(|_| ServeError::malformed_request("invalid txid"))?;

    let script_pk = address.assume_checked().script_pubkey().to_bytes();

    // Reader at current tip
    let storage = state.read().await.reader(Timestamp::from_u64(u64::MAX));

    let tx_hash = txid.to_byte_array();

    let start_ref = TxoRef {
        tx_hash,
        txo_index: 0,
    };

    let end_ref = TxoRef {
        tx_hash,
        txo_index: u32::MAX,
    };

    let range = UtxoByTxoRefKV::encode_range(Some(&start_ref), Some(&end_ref));

    let iter = storage.iter_kvs::<UtxoByTxoRefKV>(range, false);

    let mut out: Vec<RuneEdict> = Vec::new();

    for kv in iter {
        let (txo_ref, utxo) = kv?;

        // Only consider outputs controlled by the requested address
        if utxo.script != script_pk {
            continue;
        }

        let raw_runes = utxo
            .extended
            .get(&TransactionIndexer::Runes)
            .ok_or_else(|| ServeError::internal("missing runes metadata"))?;
        let runes = UtxoRunes::decode_all(raw_runes)?;

        for (rune_id, quantity) in runes {
            let rune_info = storage.get_expected::<RuneInfoByIdKV>(&rune_id)?;

            let block_hash_bytes = storage.get_expected::<HashByHeightKV>(&rune_id.block)?;
            let block_hash = BlockHash::from_byte_array(block_hash_bytes);

            let rune = Rune(rune_info.name);

            out.push(RuneEdict {
                rune_id: format!("{}", rune_id),
                amount: decimal(quantity, rune_info.divisibility),
                output: txo_ref.txo_index,
                tx_id: txid.to_string(),
                block_height: rune_id.block,
                tx_index: rune_id.tx,
                divisibility: rune_info.divisibility,
                name: rune.to_string(),
                symbol: rune_info.symbol.and_then(|s| char::from_u32(s)),
                block_hash: block_hash.to_string(),
                premine: rune_info.premine.to_string(),
            });
        }
    }

    Ok((StatusCode::OK, Json(out)))
}
