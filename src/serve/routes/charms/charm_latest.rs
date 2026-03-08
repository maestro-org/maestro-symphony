use crate::serve::AppState;
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::serve::types::{CharmUtxo, MempoolParam, ServeResponse};
use crate::serve::utils::parse_charm_path;
use crate::storage::table::Table;
use crate::sync::stages::index::indexers::core::utxo_by_txo_ref::UtxoByTxoRefKV;
use crate::sync::stages::index::indexers::custom::TransactionIndexer;
use crate::sync::stages::index::indexers::custom::charms::tables::CharmsUtxosByAppKV;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use bitcoin::{Txid, hashes::Hash};

use super::charm_util::decode_utxo_charms;

#[utoipa::path(
    tag = "Charms (Metaprotocol)",
    get,
    path = "/charms/{charm}/latest",
    params(
        ("charm" = String, Path, description = "Charm app in URI-safe form (dashes instead of slashes)", example="t-0000000000000000000000000000000000000000000000000000000000000001-0000000000000000000000000000000000000000000000000000000000000002"),
        ("mempool" = inline(Option<bool>), Query, description = "Mempool-aware"),
    ),
    responses(
        (status = 200, description = "Latest UTXO containing this charm", body = ServeResponse<CharmUtxo>),
        (status = 400, description = "Malformed query parameters"),
        (status = 404, description = "Charm not found"),
        (status = 500, description = "Internal server error"),
    )
)]
/// Latest Charm UTXO
///
/// Returns the most recently produced UTXO containing the specified charm, along with all charm
/// values in that UTXO. Useful for getting reference NFT content.
pub async fn charm_latest(
    State(state): State<AppState>,
    Query(params): Query<MempoolParam>,
    Path(charm): Path<String>,
) -> Result<impl IntoResponse, ServeError> {
    let canonical_app = parse_charm_path(&charm);
    let app_bytes = canonical_app.as_bytes().to_vec();

    let (storage, indexer_info) = state.start_reader(params.mempool).await?;

    // Range scan for this app, all heights. Reverse to get latest first.
    let range = CharmsUtxosByAppKV::encode_range(
        Some(&(app_bytes.clone(), u64::MIN)),
        Some(&(app_bytes, u64::MAX)),
    );

    let mut iter = storage.iter_kvs::<CharmsUtxosByAppKV>(range, true);

    let (key, _) = iter
        .next()
        .ok_or(ServeError::NotFound)?
        .map_err(|e| ServeError::internal(format!("{e:?}")))?;

    let utxo = storage.get_expected::<UtxoByTxoRefKV>(&key.txo_ref)?;

    let raw = utxo
        .extended
        .get(&TransactionIndexer::Charms)
        .ok_or_else(|| ServeError::internal("missing charms data"))?;

    let charms = decode_utxo_charms(raw)?;

    let resp = ServeResponse {
        data: CharmUtxo {
            tx_hash: Txid::from_byte_array(key.txo_ref.tx_hash).to_string(),
            output_index: key.txo_ref.txo_index,
            height: key.produced_height,
            satoshis: utxo.satoshis.to_string(),
            charms,
        },
        indexer_info,
    };

    Ok((StatusCode::OK, Json(resp)))
}
