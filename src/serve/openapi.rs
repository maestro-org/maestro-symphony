use std::collections::HashMap;

use super::{routes::*, types::*};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Maestro Symphony",
        version = "v0.1.0",
        description = "Maestro Symphony is a **fast**, **mempool-aware**, and **extensible** Bitcoin indexer and API server. It provides a framework for indexing UTXOs, metaprotocols, and other onchain transactions.",
        license(
            name = "Apache 2.0",
            url = "https://www.apache.org/licenses/LICENSE-2.0.txt"
        )
    ),
    // servers(
    //     (url = "...", description = "..."),
    // ),
    paths(
        addresses::all_rune_balances::addresses_all_rune_balances,
        addresses::all_rune_utxos::addresses_all_rune_utxos,
        addresses::runes_by_tx::addresses_runes_by_tx,
        addresses::specific_rune_balance::addresses_specific_rune_balance,
        addresses::specific_rune_utxos::addresses_specific_rune_utxos,
        addresses::utxos_by_address::addresses_utxos_by_address,
        runes::rune_info_batch::runes_rune_info_batch,
        runes::rune_balance_at_utxo::rune_balance_at_utxo,
    ),
    components(schemas(
        IndexerInfo,
        ChainTip,
        EstimatedBlock,
        MempoolParam,
        // --
        ServeResponse<Vec<RuneAndAmount>>,
        ServeResponse<Vec<RuneUtxo>>,
        ServeResponse<Vec<RuneEdict>>,
        ServeResponse<String>,
        ServeResponse<AddressUtxo>,
        ServeResponse<HashMap<String, Option<RuneInfo>>>,
        // ---
        RuneAndAmount,
        RuneUtxo,
        RuneEdict,
        AddressUtxo,
        RuneInfo,
    )),
)]
pub struct APIDoc;
