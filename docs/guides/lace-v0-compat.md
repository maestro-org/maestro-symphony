# Hosted-Maestro `/v0` compatibility layer

This fork adds two things on top of upstream maestro-symphony:

1. A `TxsByAddress` transaction indexer (address -> transaction history),
   the one index the upstream set lacks for wallet workloads.
2. A `/v0` API surface that mirrors the hosted Maestro Bitcoin API
   (`xbt-*.gomaestro-api.org`) payload shapes for every endpoint the Lace
   wallet uses, so a deployment of this fork replaces the hosted API by
   swapping `MAESTRO_URL_MAINNET` / `MAESTRO_URL_TESTNET` -- no wallet
   changes.

## Endpoints

| Endpoint | Backing |
| --- | --- |
| `GET /v0/addresses/{addr}/txs` | `TxsByAddress` index |
| `GET /v0/addresses/{addr}/utxos` | `UtxosByAddress` index |
| `GET /v0/mempool/addresses/{addr}/utxos` | `UtxosByAddress` index, mempool-aware reader |
| `GET /v0/rpc/general/info` | bitcoind `getblockchaininfo` |
| `GET /v0/rpc/transaction/{txid}?verbose=true` | bitcoind `getrawtransaction` (+ prevout resolution) |
| `POST /v0/rpc/transaction/submit` | bitcoind `sendrawtransaction` (201 + txid on success) |
| `GET /v0/rpc/transaction/estimatefee/{blocks}?mode=` | bitcoind `estimatesmartfee` |

Pagination uses opaque `cursor` / `next_cursor` values with `count` (max 100),
`order` (`asc`/`desc`), and inclusive `from`/`to` height bounds, per the hosted
API. Never-used addresses return 200 with empty data; an unknown transaction
returns 404. Both match observed hosted behavior.

Cursors are wire-compatible with the hosted API (unpadded url-safe base64 of
the same key layout). A cursor minted by this server resumes at its exact
position; a hosted-era cursor held by a wallet across the migration decodes
but references a key this index never minted, and resumes from its height
instead -- the whole boundary block is re-served, so history is never
skipped, and all Lace clients de-duplicate rows by transaction hash.
(Verified with a production cursor: 200, one duplicate row, zero gaps.)
No Lace client persists cursors to disk (extension/mobile hold them in a
per-session BehaviorSubject; lace-next keeps them in an unpersisted RTK
Query cache), so this path only serves sessions alive at cutover.

## Requirements

- The serve side now needs `[sync.node]` RPC access (`rpc_address`,
  `rpc_user`, `rpc_pass`) for the `/v0/rpc/*` bridges. `run` and `serve`
  modes wire it automatically from the config.
- bitcoind must run with `-txindex=1` so `getrawtransaction` resolves
  historical transactions.
- Enable the indexer in the config; adding it to an existing database
  requires a re-index from genesis (testnet4 ~30 min):

```toml
[sync.indexers]
transaction_indexers = [
    { type = "UtxosByAddress" },
    { type = "TxsByAddress" },
]
```

`TxsByAddress` is append-only (history is never deleted when outputs are
spent), so unlike `UtxosByAddress` its storage grows with chain history.
Size the volume accordingly before a mainnet sync.

## Known divergences from the hosted API

None of these are observable by the Lace clients (verified against the
wallet's four Maestro client implementations):

- `runes` and `inscriptions` arrays on UTxO entries are always empty; this
  deployment does not index metaprotocols. Enable the `Runes` indexer and
  extend the mapping if rune data is ever needed.
- `indexer_info.estimated_blocks` entries carry only `block_height`; the
  hosted API adds a `sats_per_vb` summary per estimated block.
- `data.softforks` in `general/info` is always `null` (modern bitcoind moved
  soft fork reporting to `getdeploymentinfo`); other `getblockchaininfo`
  fields pass through, so the node's field set may add fields the hosted API
  lacked.
- Volume/fee totals on verbose transactions are f64 accumulations of the
  node's BTC floats, matching the hosted values exactly (accumulated rounding
  error included). The only wire-text difference is number notation for tiny
  magnitudes: this server prints `3.68e-6` where the hosted gateway printed
  `0.0000036799999999998292` -- the same JSON number.
- Native (non-`/v0`) routes also gain `GET /addresses/{addr}/txs` in the
  upstream response envelope with the same pagination.
- The mempool-aware UTxO view is NOT capped: the hosted API silently stops at
  100 items (`next_cursor: null` at the cap), so a >100-UTxO address shows a
  truncated balance there. This server paginates the full set -- balances for
  such addresses become larger (correct) at cutover.
- `estimatefee` returns the node's real `estimatesmartfee` result; the hosted
  endpoint always returned `feerate: 0` with the target echoed. No Lace
  production code calls this endpoint (the fee market uses mempool.space).
- A malformed txid returns 400 (hosted: 500).

## Infra notes

- The `/v0` surface is unauthenticated and unthrottled, like every upstream
  Symphony route: front it with the proxy's api-key check and rate limiting
  (the hosted API enforced 200 req/s). Lace fans out one tx-detail request
  per history row.
- Pending-transaction visibility equals the local node's mempool: weak
  peering or stricter relay policy makes foreign-broadcast transactions 404
  (the wallet reads that as dropped) until they confirm. Give bitcoind good
  outbound connectivity and default relay policy.
- Wrong-network addresses return 400 as on hosted (the serve side now knows
  the configured network); the node's -txindex requirement stands.
- After deploy, validate with a known-active address: an empty history for
  it means `TxsByAddress` is missing from `transaction_indexers`.
- Cursors held by running wallet sessions at cutover (in-memory only; RTK
  Query keeps per-address cursor maps up to 1h) resume safely here via the
  height fallback (duplicates possible, skips impossible), but a clean
  cutover window or client restart avoids even that.
- This server answers no CORS preflight (hosted did, via its gateway). The
  extension and mobile contexts do not need CORS; if any plain web page ever
  consumes the API, add OPTIONS/CORS at the fronting proxy.

## Validation record (2026-09-03, testnet4)

Shape-diffed against the hosted API (`dev-maestro.lw.iog.io`) at tip ~150824,
using 8 addresses derived from the two Lace test wallets (BIP84,
m/84'/1'/0'), 2 of them with real multi-page history:

- 94 of 95 checks byte-identical: every `/addresses/{addr}/txs` page (order,
  heights, input/output flags, cursors), every confirmed and mempool-aware
  UTxO set, cursor pagination walks at count=2, all ~60 verbose transactions
  (prevout-resolved inputs, OP_RETURN outputs, totals), 404-for-unknown-tx,
  and 200-empty for unused addresses.
- The one accepted diff: our newer bitcoind adds `bits`, `target`, and `time`
  to `getblockchaininfo` (additive fields the hosted node predates).
- Values that legitimately differ between deployments (tips, confirmations,
  mempool timestamps) were compared structurally.

Additionally verified on regtest: reorg rollback of the TxsByAddress history
(orphaned transactions leave the confirmed view and reappear as mempool),
submit via `/v0/rpc/transaction/submit` (201 + txid; 400 + error detail on a
rejected transaction), and the fee-estimate fallback.
