use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Json, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use maestro_symphony_macros::{Decode, Encode};

use crate::serve::AppState;
use crate::serve::cursor::{CursorResume, decode_cursor, encode_cursor, resume_fallback};
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::routes::addresses::txs_by_address::{
    MAX_PAGE_SIZE, TxScanOpts, parse_address_script, scan_address_txs,
};
use crate::serve::types::SortOrder;
use crate::serve::v0::types::{
    V0AddressTip, V0AddressTx, V0ConfirmedUtxo, V0MempoolPaginatedResponse, V0MempoolUtxo,
    V0PaginatedResponse, V0TxsParams, V0UtxosParams,
};
use crate::storage::encdec::{Decode, Encode};
use crate::storage::kv_store::Reader;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::utxos_by_address::{
    UtxosByAddressKV, UtxosByAddressKey,
};
use crate::sync::stages::index::indexers::types::TxoRef;

const DEFAULT_DUST_THRESHOLD: u64 = 100;

/// Per-request bound on keys examined by a UTxO scan: a dust filter over a
/// dust-heavy address must not turn one page into a full-range scan. Hitting
/// the bound returns a partial page with a resume cursor.
const MAX_SCANNED_ROWS: usize = 10_000;

/// GET /v0/addresses/{address}/txs -- confirmed transaction history in the
/// hosted Maestro shape. The optional `confirmations` filter is translated to
/// an upper height bound against the indexer tip.
pub async fn txs(
    State(state): State<AppState>,
    Query(params): Query<V0TxsParams>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(false).await?;

    let (script_pk, _) = parse_address_script(&address, state.network())?;
    let tip_height = indexer_info.chain_tip.block_height;

    let mut to = params.to;
    if let Some(confirmations) = params.confirmations
        && confirmations > 0
    {
        let Some(cutoff) = (tip_height + 1).checked_sub(confirmations) else {
            let out = V0PaginatedResponse {
                data: Vec::<V0AddressTx>::new(),
                last_updated: V0AddressTip::from(&indexer_info),
                next_cursor: None,
            };
            return Ok((StatusCode::OK, Json(out)));
        };
        to = Some(to.map_or(cutoff, |t| t.min(cutoff)));
    }

    let opts = TxScanOpts {
        count: page_size(params.count)?,
        descending: matches!(params.order, Some(SortOrder::Desc)),
        cursor: params.cursor,
        from: params.from,
        to,
    };

    let page = scan_address_txs(&storage, &script_pk, &opts, tip_height)?;

    let data: Vec<V0AddressTx> = page
        .items
        .into_iter()
        .map(|(key, roles)| V0AddressTx {
            tx_hash: Txid::from_byte_array(key.tx_hash).to_string(),
            height: key.height,
            input: roles.input,
            output: roles.output,
        })
        .collect();

    let out = V0PaginatedResponse {
        data,
        last_updated: V0AddressTip::from(&indexer_info),
        next_cursor: page.next_cursor,
    };

    Ok((StatusCode::OK, Json(out)))
}

/// GET /v0/addresses/{address}/utxos -- confirmed UTxOs in the hosted Maestro
/// shape. Runes and inscriptions are always empty: this deployment indexes
/// neither, and the Lace clients ignore both fields.
pub async fn utxos(
    State(state): State<AppState>,
    Query(params): Query<V0UtxosParams>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(false).await?;

    let (script_pk, address) = parse_address_script(&address, state.network())?;
    let tip_height = indexer_info.chain_tip.block_height;
    let page = scan_address_utxos(&storage, &script_pk, &params, tip_height)?;

    let data: Vec<V0ConfirmedUtxo> = page
        .items
        .into_iter()
        .map(|item| V0ConfirmedUtxo {
            txid: item.txid,
            vout: item.vout,
            address: address.clone(),
            script_pubkey: item.script_pubkey,
            satoshis: item.satoshis.to_string(),
            confirmations: tip_height.saturating_sub(item.height) + 1,
            height: item.height,
            runes: vec![],
            inscriptions: vec![],
        })
        .collect();

    let out = V0PaginatedResponse {
        data,
        last_updated: V0AddressTip::from(&indexer_info),
        next_cursor: page.next_cursor,
    };

    Ok((StatusCode::OK, Json(out)))
}

/// GET /v0/mempool/addresses/{address}/utxos -- mempool-aware UTxO view in the
/// hosted Maestro shape: confirmed UTxOs already spent by a mempool
/// transaction are excluded, unconfirmed outputs carry `mempool: true` and
/// their estimated (pseudo-block) height.
pub async fn mempool_utxos(
    State(state): State<AppState>,
    Query(params): Query<V0UtxosParams>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, mut indexer_info) = state.start_reader(true).await?;

    let (script_pk, address) = parse_address_script(&address, state.network())?;
    let tip_height = indexer_info.chain_tip.block_height;
    let page = scan_address_utxos(&storage, &script_pk, &params, tip_height)?;

    let data: Vec<V0MempoolUtxo> = page
        .items
        .into_iter()
        .map(|item| V0MempoolUtxo {
            txid: item.txid,
            vout: item.vout,
            address: address.clone(),
            script_pubkey: item.script_pubkey,
            satoshis: item.satoshis.to_string(),
            height: item.height,
            mempool: item.height > tip_height,
            runes: vec![],
            inscriptions: vec![],
        })
        .collect();

    if data.is_empty() {
        indexer_info.mempool_timestamp = None;
    }

    let out = V0MempoolPaginatedResponse {
        data,
        indexer_info,
        next_cursor: page.next_cursor,
    };

    Ok((StatusCode::OK, Json(out)))
}

fn page_size(count: Option<usize>) -> Result<usize, ServeError> {
    let count = count.unwrap_or(MAX_PAGE_SIZE);
    if count == 0 || count > MAX_PAGE_SIZE {
        return Err(ServeError::malformed_request(format!(
            "count must be between 1 and {MAX_PAGE_SIZE}"
        )));
    }
    Ok(count)
}

/// Position of the next UTxO to return within one script's key range.
/// Serialized (base64url) as the opaque pagination cursor.
#[derive(Encode, Decode, Debug)]
struct UtxoCursor {
    height: u64,
    txo_ref: TxoRef,
}

struct RawEncoded(Vec<u8>);

impl Encode for RawEncoded {
    fn encode(&self) -> Vec<u8> {
        self.0.clone()
    }
}

struct UtxoItem {
    txid: String,
    vout: u32,
    script_pubkey: String,
    satoshis: u64,
    height: u64,
}

struct UtxoPage {
    items: Vec<UtxoItem>,
    next_cursor: Option<String>,
}

fn scan_address_utxos(
    storage: &Reader,
    script_pk: &[u8],
    params: &V0UtxosParams,
    tip_height: u64,
) -> Result<UtxoPage, ServeError> {
    let count = page_size(params.count)?;
    let descending = matches!(params.order, Some(SortOrder::Desc));

    let dust_threshold = match params.filter_dust {
        Some(true) => params
            .filter_dust_threshold
            .unwrap_or(DEFAULT_DUST_THRESHOLD),
        _ => 0,
    };

    let script_enc = script_pk.to_vec().encode();

    let resume = params
        .cursor
        .as_ref()
        .map(|c| {
            let raw =
                decode_cursor(c).ok_or_else(|| ServeError::malformed_request("invalid cursor"))?;
            let parsed = UtxoCursor::decode_all(&raw)
                .map_err(|_| ServeError::malformed_request("invalid cursor"))?;

            let key = UtxosByAddressKey {
                script: script_pk.to_vec(),
                produced_height: parsed.height,
                txo_ref: parsed.txo_ref,
            };

            Ok::<CursorResume, ServeError>(if storage.get::<UtxosByAddressKV>(&key)?.is_some() {
                CursorResume::Exact(parsed.encode())
            } else {
                resume_fallback(parsed.height, tip_height, descending)
            })
        })
        .transpose()?;

    let mut start = script_enc.clone();
    let mut end = script_enc.clone();

    match (&resume, descending) {
        (Some(CursorResume::Exact(cursor)), false) => start.extend_from_slice(cursor),
        (Some(CursorResume::Exact(cursor)), true) => {
            end.extend_from_slice(cursor);
            end.push(0x00);
        }
        (Some(CursorResume::HeightOnly(height)), false) => {
            start.extend_from_slice(&height.encode());
        }
        (Some(CursorResume::HeightOnly(height)), true) => {
            end.extend_from_slice(&height.saturating_add(1).encode());
        }
        (Some(CursorResume::Top), true) => end.extend_from_slice(&u64::MAX.encode()),
        (Some(CursorResume::Top), false) => {}
        (None, _) => {}
    }

    if (resume.is_none() || descending)
        && let Some(from) = params.from
    {
        start.extend_from_slice(&from.encode());
    }

    if resume.is_none() || !descending {
        match params.to {
            Some(to) => end.extend_from_slice(&to.saturating_add(1).encode()),
            None => end.extend_from_slice(&u64::MAX.encode()),
        }
    }

    let range = UtxosByAddressKV::encode_range(Some(&RawEncoded(start)), Some(&RawEncoded(end)));

    let mut iter = storage.iter_kvs::<UtxosByAddressKV>(range, descending);

    let mut items = Vec::with_capacity(count);
    let mut next_cursor = None;
    let mut examined = 0usize;

    for kv in iter.by_ref() {
        let (key, _): (UtxosByAddressKey, ()) = kv?;

        let mint_boundary = |key: &UtxosByAddressKey| {
            encode_cursor(
                &UtxoCursor {
                    height: key.produced_height,
                    txo_ref: key.txo_ref,
                }
                .encode(),
            )
        };

        examined += 1;
        if examined > MAX_SCANNED_ROWS {
            next_cursor = Some(mint_boundary(&key));
            break;
        }

        let Some(utxo) = storage.get_maybe::<UtxoByTxoRefKV>(&key.txo_ref)? else {
            continue;
        };

        if utxo.satoshis < dust_threshold {
            continue;
        }

        if items.len() == count {
            next_cursor = Some(mint_boundary(&key));
            break;
        }

        items.push(UtxoItem {
            txid: Txid::from_byte_array(key.txo_ref.tx_hash).to_string(),
            vout: key.txo_ref.txo_index,
            script_pubkey: hex::encode(&utxo.script),
            satoshis: utxo.satoshis,
            height: key.produced_height,
        });
    }

    Ok(UtxoPage { items, next_cursor })
}
