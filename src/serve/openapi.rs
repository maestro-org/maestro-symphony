use super::{routes::*, types::*};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Maestro Symphony",
        version = "v0.0.1",
        description = "Maestro Symphony is a fast, mempool-aware Bitcoin indexer and API server. It supports mainnet and testnet4, providing data for UTXOs, metaprotocols and much more. Designed for developers who need reliable, mempool-aware blockchain data.",
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
        runes::rune_info::runes_rune_info,
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
        ServeResponse<RuneAndAmount>,
        ServeResponse<AddressUtxo>,
        ServeResponse<RuneInfoBatch>,
        // ---
        RuneAndAmount,
        RuneUtxo,
        RuneEdict,
        AddressUtxo,
        RuneInfo,
        RuneInfoBatch,
    )),
)]
pub struct APIDoc;
