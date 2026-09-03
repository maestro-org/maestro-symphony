use crate::serve::cursor::{CursorResume, decode_cursor, encode_cursor, resume_fallback};
use crate::serve::error::ServeError;
use crate::serve::routes::addresses::AppState;
use crate::serve::types::{AddressTx, PaginatedServeResponse, SortOrder, TxsByAddressParams};
use crate::storage::encdec::{Decode, Encode};
use crate::storage::kv_store::Reader;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::custom::txs_by_address::{
    TxAddressRoles, TxsByAddressKV, TxsByAddressKey,
};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Json, extract::State, response::IntoResponse};
use bitcoin::Txid;
use bitcoin::hashes::Hash;
use maestro_symphony_macros::{Decode, Encode};
use std::str::FromStr;

pub const DEFAULT_PAGE_SIZE: usize = 100;
pub const MAX_PAGE_SIZE: usize = 100;

/// Position of the next item to return, relative to one script's key range.
/// Serialized (base64url) as the opaque pagination cursor -- the same byte
/// layout the hosted API used.
#[derive(Encode, Decode, Debug)]
struct TxCursor {
    height: u64,
    tx_index: u32,
    tx_hash: [u8; 32],
}

/// Raw pre-encoded bytes usable as an encode_range bound.
struct RawEncoded(Vec<u8>);

impl Encode for RawEncoded {
    fn encode(&self) -> Vec<u8> {
        self.0.clone()
    }
}

pub struct TxScanOpts {
    pub count: usize,
    pub descending: bool,
    pub cursor: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
}

impl TxScanOpts {
    pub fn from_params(params: &TxsByAddressParams) -> Result<Self, ServeError> {
        let count = params.count.unwrap_or(DEFAULT_PAGE_SIZE);
        if count == 0 || count > MAX_PAGE_SIZE {
            return Err(ServeError::malformed_request(format!(
                "count must be between 1 and {MAX_PAGE_SIZE}"
            )));
        }

        Ok(Self {
            count,
            descending: matches!(params.order, Some(SortOrder::Desc)),
            cursor: params.cursor.clone(),
            from: params.from,
            to: params.to,
        })
    }
}

pub struct AddressTxsPage {
    pub items: Vec<(TxsByAddressKey, TxAddressRoles)>,
    pub next_cursor: Option<String>,
}

/// Scans one script's transaction history page. The cursor marks the next
/// item to return (inclusive) and always advances in the scan direction, so
/// it overrides `from` (ascending) or `to` (descending) on its side.
/// `tip_height` is the confirmed tip of the reader: rows above it are the
/// mempool overlay, which is rebuilt on every snapshot and needs the special
/// cursor handling documented in serve::cursor.
pub fn scan_address_txs(
    storage: &Reader,
    script_pk: &[u8],
    opts: &TxScanOpts,
    tip_height: u64,
) -> Result<AddressTxsPage, ServeError> {
    let script_enc = script_pk.to_vec().encode();

    let resume = opts
        .cursor
        .as_ref()
        .map(|c| {
            let raw =
                decode_cursor(c).ok_or_else(|| ServeError::malformed_request("invalid cursor"))?;
            let parsed = TxCursor::decode_all(&raw)
                .map_err(|_| ServeError::malformed_request("invalid cursor"))?;

            let key = TxsByAddressKey {
                script: script_pk.to_vec(),
                height: parsed.height,
                tx_index: parsed.tx_index,
                tx_hash: parsed.tx_hash,
            };

            Ok::<CursorResume, ServeError>(if storage.get::<TxsByAddressKV>(&key)?.is_some() {
                CursorResume::Exact(parsed.encode())
            } else {
                resume_fallback(parsed.height, tip_height, opts.descending)
            })
        })
        .transpose()?;

    let mut start = script_enc.clone();
    let mut end = script_enc.clone();

    match (&resume, opts.descending) {
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

    if (resume.is_none() || opts.descending)
        && let Some(from) = opts.from
    {
        start.extend_from_slice(&from.encode());
    }

    if resume.is_none() || !opts.descending {
        match opts.to {
            Some(to) => end.extend_from_slice(&to.saturating_add(1).encode()),
            None => end.extend_from_slice(&u64::MAX.encode()),
        }
    }

    let range = TxsByAddressKV::encode_range(Some(&RawEncoded(start)), Some(&RawEncoded(end)));

    let mut iter = storage.iter_kvs::<TxsByAddressKV>(range, opts.descending);

    let mut items = Vec::with_capacity(opts.count);
    let mut next_cursor = None;

    for kv in iter.by_ref() {
        let (key, roles) = kv?;

        if items.len() == opts.count {
            next_cursor = Some(encode_cursor(
                &TxCursor {
                    height: key.height,
                    tx_index: key.tx_index,
                    tx_hash: key.tx_hash,
                }
                .encode(),
            ));
            break;
        }

        items.push((key, roles));
    }

    Ok(AddressTxsPage { items, next_cursor })
}

/// Parses and network-checks an address, returning its script pubkey and
/// its canonical string form (bech32 inputs normalize to lowercase, matching
/// the hosted API's echo of the address in response rows).
pub fn parse_address_script(
    address: &str,
    network: Option<bitcoin::Network>,
) -> Result<(Vec<u8>, String), ServeError> {
    let address = bitcoin::Address::from_str(address)
        .map_err(|_| ServeError::malformed_request("invalid address"))?;

    let address = match network {
        Some(network) => address
            .require_network(network)
            .map_err(|_| ServeError::malformed_request("address not valid for network"))?,
        None => address.assume_checked(),
    };

    Ok((address.script_pubkey().to_bytes(), address.to_string()))
}

#[utoipa::path(
    tag = "Addresses",
    get,
    path = "/addresses/{address}/txs",
    params(
        ("address" = String, Path, description = "Bitcoin address", example="tb1qphcdyah2e4vtpxn56hsz3p6kapg90pl4x525kc"),

        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
        ("count" = inline(Option<usize>), Query, description = "Max results per page (default 100, max 100)"),
        ("order" = inline(Option<String>), Query, description = "Sort order by height: asc or desc (default asc)"),
        ("cursor" = inline(Option<String>), Query, description = "Pagination cursor from a previous response"),
        ("from" = inline(Option<u64>), Query, description = "Include results from this height (inclusive)"),
        ("to" = inline(Option<u64>), Query, description = "Include results up to this height (inclusive)"),
    ),
    responses(
        (
            status = 200,
            description = "Requested data",
            body = PaginatedServeResponse<Vec<AddressTx>>,
            example = json!(serde_json::Value::from_str(EXAMPLE_RESPONSE).unwrap())
        ),
        (status = 400, description = "Malformed query parameters"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Transactions by Address
///
/// Returns the transactions in which the address controlled an input or an
/// output, oldest first by default (order=desc for newest first), paginated
/// via an opaque cursor.
pub async fn addresses_txs_by_address(
    State(state): State<AppState>,
    Query(params): Query<TxsByAddressParams>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

    let (script_pk, _) = parse_address_script(&address, state.network())?;
    let opts = TxScanOpts::from_params(&params)?;

    let page = scan_address_txs(
        &storage,
        &script_pk,
        &opts,
        indexer_info.chain_tip.block_height,
    )?;

    let data: Vec<AddressTx> = page
        .items
        .into_iter()
        .map(|(key, roles)| AddressTx {
            tx_hash: Txid::from_byte_array(key.tx_hash).to_string(),
            height: key.height,
            input: roles.input,
            output: roles.output,
        })
        .collect();

    let out = PaginatedServeResponse {
        data,
        indexer_info,
        next_cursor: page.next_cursor,
    };

    Ok((StatusCode::OK, Json(out)))
}

static EXAMPLE_RESPONSE: &str = r##"{
  "data": [
    {
      "tx_hash": "50cc4d0dc76f040df16c5c841ee5abe4718e36b2cd46df3cba3005598e2e0021",
      "height": 147047,
      "input": true,
      "output": true
    }
  ],
  "indexer_info": {
    "chain_tip": {
      "block_hash": "0000000000c05511f3ee1f8671b02362629544ad7ef7ffed90e93f2c3c0ae286",
      "block_height": 150814
    },
    "mempool_timestamp": null,
    "estimated_blocks": []
  },
  "next_cursor": null
}"##;
