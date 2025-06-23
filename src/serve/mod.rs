use crate::error::Error;
use crate::serve::error::ServeError;
use crate::serve::reader_wrapper::ServeReaderHelper;
use crate::storage::encdec::prefix_key_range;
use crate::storage::kv_store::{Reader, StorageHandler};
use crate::storage::table::Table;
use crate::storage::timestamp::Timestamp;
use crate::sync::stages::index::indexers::core::hash_by_height::HashByHeightKV;
use crate::sync::stages::index::indexers::core::timestamps::{PointKind, TimestampsKV};
use axum::body::Body;
use axum::extract::Query;
use axum::http::Request;
use axum::{
    Json, Router,
    extract::State,
    middleware::{self, Next},
    response::IntoResponse,
    routing::get,
};
use axum_server::Server;
use bitcoin::BlockHash;
use bitcoin::hashes::Hash;
use rocksdb::{IteratorMode, ReadOptions};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

mod error;
mod reader_wrapper;
mod routes;
mod utils;

pub static DEFAULT_SERVE_ADDRESS: &str = "0.0.0.0:8080";

#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    pub address: Option<String>,
}

#[derive(Clone)]
pub struct AppState(Arc<RwLock<StorageHandler>>);

impl AppState {
    pub async fn start_reader(&self, mempool: bool) -> Result<Reader, ServeError> {
        let storage = self.0.read().await;

        let latest_reader = storage.reader(Timestamp::from_u64(u64::MAX));

        let confirmed_point = latest_reader.get_expected::<TimestampsKV>(&PointKind::Confirmed)?;

        // use mempool timestamp if mempool true && mempool timestamp found && mempool point
        // is chained on current confirmed tip
        let reader = if mempool {
            let mempool_point = latest_reader.get_maybe::<TimestampsKV>(&PointKind::Mempool)?;

            if let Some(mempool_point) = mempool_point {
                if mempool_point.tip_hash != confirmed_point.tip_hash {
                    // mempool blocks not chained on current confirmed tip
                    storage.reader(Timestamp::from_u64(confirmed_point.timestamp))
                } else {
                    storage.reader(Timestamp::from_u64(mempool_point.timestamp))
                }
            } else {
                // no mempool timestamp found
                storage.reader(Timestamp::from_u64(confirmed_point.timestamp))
            }
        } else {
            // mempool: false
            storage.reader(Timestamp::from_u64(confirmed_point.timestamp))
        };

        Ok(reader)
    }
}

async fn auto_refresh(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> impl IntoResponse {
    // Try to refresh the database if in read-only mode (secondary rocksdb instance
    // need to be manually told to catch up to the primary)
    let should_refresh = state.0.read().await.is_read_only();

    if should_refresh {
        let mut storage_handler = state.0.write().await;

        if let Err(e) = storage_handler.try_refresh_read_only_data() {
            // Log warning but continue with potentially stale data
            warn!("Failed to refresh read-only database: {}", e);
        }

        // release lock
        drop(storage_handler)
    }

    // Continue with the request
    next.run(request).await
}

pub async fn run(db: StorageHandler, address: &str) -> Result<(), Error> {
    let app_state = AppState(Arc::new(RwLock::new(db)));

    let app = Router::new()
        .route("/", get(root))
        .route("/dump", get(dump))
        .route("/tip", get(tip))
        .nest("/addresses", routes::addresses::router())
        .nest("/runes", routes::runes::router())
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            auto_refresh,
        ))
        .with_state(app_state);

    let addr = match address.parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("Failed to parse server address '{}': {}", address, e);
            return Err(Error::Config(e.to_string()));
        }
    };

    info!("api listening on {}...", addr);

    Server::bind(addr)
        .serve(app.into_make_service())
        .await
        .map_err(|e| Error::Config(e.to_string()))?;

    Ok(())
}

async fn root() -> &'static str {
    "Symphony API Server"
}

#[derive(Debug, Deserialize)]
pub struct DumpParam {
    pub prefix: Option<String>,
}

// Dump all KVs (temporary debugging)
async fn dump(State(state): State<AppState>, Query(param): Query<DumpParam>) -> impl IntoResponse {
    let storage_handler = state.0.read().await;
    let cf = storage_handler.cf_handle();

    let mut read_opts = ReadOptions::default();
    read_opts.set_timestamp(Timestamp::from_u64(u64::MAX).as_rocksdb_ts());

    if let Some(prefix) = param.prefix {
        let prefix = hex::decode(&prefix).unwrap();

        let range = prefix_key_range(&prefix);
        read_opts.set_iterate_range(range);
    }

    let mut out = vec![];

    for x in storage_handler
        .db
        .iterator_cf_opt(&cf, read_opts, IteratorMode::Start)
    {
        let x = x.unwrap();
        out.push(format!("{} -> {}", hex::encode(&x.0), hex::encode(&x.1)));
    }

    out.join("\n").into_response()
}

async fn tip(State(state): State<AppState>) -> impl IntoResponse {
    let storage = state.0.read().await.reader(Timestamp::from_u64(u64::MAX)); // TODO

    let range = HashByHeightKV::encode_range(None::<&()>, None::<&()>);

    let res = storage.iter_kvs::<HashByHeightKV>(range, true).next();

    let json = match res {
        Some(Ok(x)) => Json(json!({
            "height": x.0,
            "hash": BlockHash::from_byte_array(x.1).to_string()
        })),
        _ => Json(json!({})),
    };

    json.into_response()
}

#[derive(Deserialize)]
pub struct QueryParams {
    #[serde(default)]
    mempool: bool,
}
