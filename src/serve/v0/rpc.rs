use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Json, response::IntoResponse};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::serve::AppState;
use crate::serve::error::ServeError;
use crate::serve::v0::node_rpc::NodeRpc;
use crate::serve::v0::types::{V0BlockPointer, V0DataResponse, V0FeeEstimate, V0FeeModeParam};

/// GET /v0/rpc/general/info -- getblockchaininfo passthrough with the hosted
/// Maestro envelope. `last_updated` is the INDEXER tip (not the node tip), so
/// a client that reads the tip here and then queries indexed data sees a
/// consistent view.
pub async fn general_info(State(state): State<AppState>) -> Result<impl IntoResponse, ServeError> {
    let (_, indexer_info) = state.start_reader(false).await?;

    let mut info = state
        .node()?
        .call("getblockchaininfo", vec![])
        .await
        .map_err(|e| e.into_serve_error())?;

    if let Some(obj) = info.as_object_mut() {
        obj.entry("softforks").or_insert(Value::Null);

        if let Some(warnings) = obj.get("warnings").and_then(Value::as_array) {
            let joined = warnings
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ");
            obj.insert("warnings".into(), json!(joined));
        }
    }

    let out = V0DataResponse {
        data: info,
        last_updated: V0BlockPointer::from(&indexer_info),
    };

    Ok((StatusCode::OK, Json(out)))
}

/// GET /v0/rpc/transaction/{txid} -- getrawtransaction bridged to the hosted
/// Maestro verbose shape: input addresses/values resolved from prevouts, flat
/// output addresses, block height, and volume/fee totals. 404 for a
/// transaction the node does not know.
pub async fn transaction(
    State(state): State<AppState>,
    Path(txid): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    if txid.len() != 64 || !txid.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ServeError::malformed_request("invalid transaction id"));
    }

    let (_, indexer_info) = state.start_reader(false).await?;
    let node = state.node()?;

    let raw = get_raw_transaction_verbose(node, &txid).await?;
    let data = build_maestro_tx(node, &raw).await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": data,
            "last_updated": V0BlockPointer::from(&indexer_info),
        })),
    ))
}

/// POST /v0/rpc/transaction/submit -- sendrawtransaction. The hosted API
/// contract the wallet depends on: the body is a JSON-encoded hex string and
/// ONLY a 201 with the txid as JSON body counts as a successful broadcast.
pub async fn submit(
    State(state): State<AppState>,
    Json(raw_hex): Json<String>,
) -> Result<impl IntoResponse, ServeError> {
    let node = state.node()?;

    match node.call("sendrawtransaction", vec![json!(raw_hex)]).await {
        Ok(txid) => Ok((StatusCode::CREATED, Json(txid)).into_response()),
        // Only a genuine transaction rejection is the client's fault. A
        // transport failure or any node-side state (warmup, IBD, internal
        // fault) must surface as 5xx: a 400 would make the wallet treat an
        // unreachable node as a permanently rejected transaction.
        Err(e) if e.is_transaction_rejection() => {
            Ok((StatusCode::BAD_REQUEST, Json(json!({ "error": e.message }))).into_response())
        }
        Err(e) => Err(e.into_serve_error()),
    }
}

/// GET /v0/rpc/transaction/estimatefee/{blocks} -- estimatesmartfee in the
/// hosted Maestro envelope. Mirrors the hosted behavior of reporting a zero
/// feerate when the node has no estimate (common on quiet testnets).
pub async fn estimate_fee(
    State(state): State<AppState>,
    Query(params): Query<V0FeeModeParam>,
    Path(blocks): Path<u64>,
) -> Result<impl IntoResponse, ServeError> {
    let (_, indexer_info) = state.start_reader(false).await?;

    let mut rpc_params = vec![json!(blocks)];
    if let Some(mode) = params.mode {
        let mode = mode.to_uppercase();
        if mode == "CONSERVATIVE" || mode == "ECONOMICAL" {
            rpc_params.push(json!(mode));
        }
    }

    let estimate = state
        .node()?
        .call("estimatesmartfee", rpc_params)
        .await
        .map_err(|e| {
            if e.is_invalid_parameter() {
                ServeError::malformed_request(e.message)
            } else {
                e.into_serve_error()
            }
        })?;

    let feerate = estimate.get("feerate").filter(|f: &&Value| f.is_number());

    let out = V0DataResponse {
        data: V0FeeEstimate {
            feerate: feerate.cloned().unwrap_or(json!(0)),
            blocks: match feerate {
                Some(_) => estimate
                    .get("blocks")
                    .and_then(Value::as_u64)
                    .unwrap_or(blocks),
                None => blocks,
            },
        },
        last_updated: V0BlockPointer::from(&indexer_info),
    };

    Ok((StatusCode::OK, Json(out)))
}

/// Fetches the verbose transaction, preferring verbosity 2 (inlined prevouts)
/// and falling back to verbosity 1 for nodes that do not support it.
async fn get_raw_transaction_verbose(node: &NodeRpc, txid: &str) -> Result<Value, ServeError> {
    match node
        .call("getrawtransaction", vec![json!(txid), json!(2)])
        .await
    {
        Ok(raw) => Ok(raw),
        Err(e) if e.is_not_found() => Err(ServeError::NotFound),
        Err(_) => node
            .call("getrawtransaction", vec![json!(txid), json!(1)])
            .await
            .map_err(|e| e.into_serve_error()),
    }
}

/// Prefetches the source transactions of every input lacking an inlined
/// verbosity-2 `prevout` (the mempool-transaction case), one lookup per
/// UNIQUE source txid. A source the node cannot find degrades that input to
/// the empty-prevout path -- it must never surface as 404, which the wallet
/// reads as "this transaction does not exist".
/// Per-request bound on prevout source lookups: each is a blocking node
/// round-trip, and a mempool transaction can have thousands of inputs.
const MAX_PREVOUT_LOOKUPS: usize = 500;

async fn prefetch_prevout_sources(
    node: &NodeRpc,
    raw_vins: &[Value],
) -> Result<HashMap<String, Value>, ServeError> {
    let mut sources = HashMap::new();

    for raw_vin in raw_vins {
        if raw_vin.get("prevout").is_some_and(Value::is_object) || raw_vin.get("coinbase").is_some()
        {
            continue;
        }
        let Some(txid) = raw_vin.get("txid").and_then(Value::as_str) else {
            continue;
        };
        if sources.contains_key(txid) {
            continue;
        }
        if sources.len() >= MAX_PREVOUT_LOOKUPS {
            return Err(ServeError::internal(format!(
                "transaction exceeds {MAX_PREVOUT_LOOKUPS} prevout lookups"
            )));
        }

        match node
            .call("getrawtransaction", vec![json!(txid), json!(1)])
            .await
        {
            Ok(source) => {
                sources.insert(txid.to_string(), source);
            }
            Err(e) if e.is_not_found() => {}
            Err(e) => {
                return Err(ServeError::internal(format!(
                    "prevout lookup: {}",
                    e.message
                )));
            }
        }
    }

    Ok(sources)
}

/// Resolves one input's previous output, from the inlined verbosity-2
/// `prevout` when present, otherwise from the prefetched source transaction.
fn resolve_prevout(vin: &Value, sources: &HashMap<String, Value>) -> Option<Value> {
    if let Some(prevout) = vin.get("prevout")
        && prevout.is_object()
    {
        return Some(prevout.clone());
    }

    let txid = vin.get("txid").and_then(Value::as_str)?;
    let vout = vin.get("vout").and_then(Value::as_u64)?;

    sources
        .get(txid)?
        .get("vout")
        .and_then(|outs| outs.get(vout as usize))
        .cloned()
}

fn btc_amount(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

async fn build_maestro_tx(node: &NodeRpc, raw: &Value) -> Result<Value, ServeError> {
    let empty = vec![];
    let raw_vins = raw.get("vin").and_then(Value::as_array).unwrap_or(&empty);
    let raw_vouts = raw.get("vout").and_then(Value::as_array).unwrap_or(&empty);

    let prevout_sources = prefetch_prevout_sources(node, raw_vins).await?;

    let mut vin = Vec::with_capacity(raw_vins.len());
    let mut input_addresses = Vec::new();
    let mut input_volume: f64 = 0.0;
    let mut is_coinbase = false;

    for raw_vin in raw_vins {
        let script_sig = raw_vin
            .get("scriptSig")
            .cloned()
            .unwrap_or_else(|| json!({ "asm": "", "hex": "" }));
        let witness = raw_vin.get("txinwitness").cloned().unwrap_or(Value::Null);
        let sequence = raw_vin.get("sequence").cloned().unwrap_or(json!(0));

        if let Some(coinbase) = raw_vin.get("coinbase").and_then(Value::as_str) {
            is_coinbase = true;
            vin.push(json!({
                "script_type": "",
                "address": "",
                "value": 0.0,
                "coinbase": coinbase,
                "txid": "",
                "vout": 0,
                "scriptSig": script_sig,
                "txinwitness": witness,
                "sequence": sequence,
            }));
            continue;
        }

        let prevout = resolve_prevout(raw_vin, &prevout_sources);

        let spk = prevout
            .as_ref()
            .and_then(|p| p.get("scriptPubKey"))
            .cloned()
            .unwrap_or(json!({}));

        let address = spk
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let script_type = spk
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let value = prevout
            .as_ref()
            .and_then(|p| p.get("value"))
            .map(btc_amount)
            .unwrap_or(0.0);

        input_volume += value;
        input_addresses.push(Value::String(address.clone()));

        vin.push(json!({
            "script_type": script_type,
            "address": address,
            "value": value,
            "coinbase": "",
            "txid": raw_vin.get("txid").cloned().unwrap_or(json!("")),
            "vout": raw_vin.get("vout").cloned().unwrap_or(json!(0)),
            "scriptSig": script_sig,
            "txinwitness": witness,
            "sequence": sequence,
        }));
    }

    let mut vout = Vec::with_capacity(raw_vouts.len());
    let mut output_addresses = Vec::new();
    let mut output_volume: f64 = 0.0;

    for raw_vout in raw_vouts {
        let mut spk = raw_vout.get("scriptPubKey").cloned().unwrap_or(json!({}));
        let address = spk
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let script_type = if address.is_empty() {
            String::new()
        } else {
            spk.get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };

        if let Some(obj) = spk.as_object_mut() {
            obj.entry("address").or_insert(json!(""));
        }

        let value = raw_vout.get("value").map(btc_amount).unwrap_or(0.0);

        output_volume += value;
        if !address.is_empty() {
            output_addresses.push(Value::String(address.clone()));
        }

        vout.push(json!({
            "script_type": script_type,
            "address": address,
            "value": value,
            "n": raw_vout.get("n").cloned().unwrap_or(json!(0)),
            "scriptPubKey": spk,
        }));
    }

    let blockhash = raw.get("blockhash").and_then(Value::as_str);

    let blockheight = match blockhash {
        Some(hash) => node
            .call("getblockheader", vec![json!(hash)])
            .await
            .map_err(|e| e.into_serve_error())?
            .get("height")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        None => 0,
    };

    let confirmations = raw
        .get("confirmations")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let blocktime = raw.get("blocktime").and_then(Value::as_u64).unwrap_or(0);
    let time = raw.get("time").and_then(Value::as_u64).unwrap_or(blocktime);

    // f64 accumulation, not exact satoshi arithmetic: the hosted API sums
    // the node's BTC floats, and byte-fidelity means matching its values,
    // accumulated rounding error included.
    let total_fees = if is_coinbase {
        0.0
    } else {
        input_volume - output_volume
    };

    Ok(json!({
        "input_addresses": if is_coinbase { Value::Null } else { json!(input_addresses) },
        "output_addresses": output_addresses,
        "txid": raw.get("txid").cloned().unwrap_or(json!("")),
        "hash": raw.get("hash").cloned().unwrap_or(json!("")),
        "version": raw.get("version").cloned().unwrap_or(json!(0)),
        "size": raw.get("size").cloned().unwrap_or(json!(0)),
        "vsize": raw.get("vsize").cloned().unwrap_or(json!(0)),
        "weight": raw.get("weight").cloned().unwrap_or(json!(0)),
        "locktime": raw.get("locktime").cloned().unwrap_or(json!(0)),
        "vin": vin,
        "vout": vout,
        "hex": raw.get("hex").cloned().unwrap_or(json!("")),
        "blockhash": raw.get("blockhash").cloned().unwrap_or(json!("")),
        "blockheight": blockheight,
        "blocktime": blocktime,
        "confirmations": confirmations,
        "time": time,
        "total_input_volume": input_volume,
        "total_output_volume": output_volume,
        "total_fees": total_fees,
    }))
}
