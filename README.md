🎵 ₿ _from hash to harmony: an orchestrated Bitcoin indexing suite_ ₿ 🎵

# [Maestro Symphony](https://www.gomaestro.org/)

[![CI](https://img.shields.io/github/actions/workflow/status/maestro-org/maestro-symphony/ci.yml?label=CI&logo=github&style=for-the-badge)](https://github.com/maestro-org/maestro-symphony/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/maestro-symphony?label=crates.io&logo=rust&style=for-the-badge)](https://crates.io/crates/maestro-symphony)
[![Docs](https://img.shields.io/badge/docs-website-C53DD8?style=for-the-badge)](https://docs.gomaestro.org/)
[![Discord](https://img.shields.io/discord/950173135838273556?label=discord&logo=discord&style=for-the-badge)](https://discord.gg/SJgkEje7)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg?style=for-the-badge)](./LICENSE)

<img src="https://github.com/maestro-org/maestro-symphony/raw/main/logo/logo.png" width="100">

---

## Overview

Maestro Symphony is a **fast**, **mempool-aware**, and **extensible** Bitcoin indexer and API server. It provides a framework for indexing UTXOs, metaprotocols, and any other onchain activity.

---

## Core Features

<details>
<summary><strong>Supported Networks</strong></summary>

-   mainnet
-   testnet4
-   regtest

</details>

<details>
<summary><strong>Indexers</strong></summary>

-   Runes
-   Transaction count by address
-   UTXOs by address

</details>

**Endpoints:** [OpenAPI](docs/openapi.json)

**Mempool Awareness:** Query any endpoint with `?mempool=true` to include pending transactions.

**Rollback Handling:** Always maintains an index of the longest chain.

---

## Prerequisites

-   [Bitcoin Core (22+)](https://hub.docker.com/r/bitcoin/bitcoin) with RPC and P2P access
-   [Rust (stable)](https://www.rust-lang.org/tools/install)

### Optional Tools

-   [Docker](https://docs.docker.com/compose/install/)
-   [mise](https://mise.jdx.dev/getting-started.html)

### Deployment Requirements

<details>
<summary><strong>Testnet4</strong></summary>

-   Disk: 1 GB
-   CPU: 2 cores
-   RAM: 4 GB
-   Sync time: ~6 hours

</details>

<details>
<summary><strong>Mainnet</strong></summary>

-   Disk: 24 GB
-   CPU: 4 cores
-   RAM: 16 GB
-   Sync time: ~4 days

</details>

**NOTE**: Deployment requirements are subject to change with new indexers and API endpoints.

---

## Configuration

Below is a table describing the main configuration options for `maestro-symphony`. See the example configuration for context.

| Section           | Key/Field              | Description                                           | Example Value             |
| ----------------- | ---------------------- | ----------------------------------------------------- | ------------------------- |
| _root_            | `db_path`              | Path to the database directory                        | `"tmp/symphony"`          |
| `[sync.node]`     | `p2p_address`          | Host/IP and port for P2P connection to Bitcoin node   | `"localhost:8333"`        |
|                   | `rpc_address`          | URL of your Bitcoin node's RPC endpoint               | `"http://localhost:8332"` |
|                   | `rpc_user`             | RPC username for your Bitcoin node                    | `"bitcoin"`               |
|                   | `rpc_pass`             | RPC password for your Bitcoin node                    | `"password"`              |
| `[sync]`          | `network`              | Bitcoin network to connect to (`mainnet`, `testnet4`) | `"mainnet"`               |
|                   | `safe_mode`            | Enable safe mode for sync (recommended)               | `true`                    |
|                   | `max_rollback`         | Maximum blocks to roll back on reorg                  | `32`                      |
|                   | `mempool`              | Enable mempool awareness                              | `true`                    |
| `[sync.indexers]` | `transaction_indexers` | List of enabled indexers and their options            | See example below         |
| `[server]`        | `address`              | Address and port for API server to listen on          | `"0.0.0.0:8080"`          |

See [examples](examples/) to quickly get started.

---

## Running Locally

Optionally, use `mise` to easily set up your environment:

```bash
mise install
```

### Build

```bash
make build
```

### Sync & Serve

```bash
make run [CONFIG=examples/testnet.toml]
```

### Sync Only

```bash
make sync [CONFIG=examples/testnet.toml]
```

### Serve Only

```bash
make serve [CONFIG=examples/testnet.toml]
```

### Generate OpenAPI

```bash
make openapi
```

---

## Running with Docker

### Start

```bash
make compose-up
```

### Stop

```bash
make compose-down
```

---

## Endpoint Examples

### Rune UTXOs by Address

```bash
curl -X GET "http://localhost:8080/addresses/tb1pn9dzakm6egrv90c9gsgs63axvmn6ydwemrpuwljnmz9qdk38ueqsqae936/runes/utxos" | jq .
```

```json
{
    "data": [
        {
            "tx_hash": "63937d48e35d15a7c5530469210c202104cc94a945cc848554f336b3f4f24121",
            "output_index": 1,
            "height": 30562,
            "satoshis": "10000",
            "runes": [
                {
                    "id": "30562:50",
                    "amount": "100000000"
                }
            ]
        }
    ],
    "indexer_info": {
        "chain_tip": {
            "block_hash": "000000002ec229e75c52e8e9adf95149fdde167b59c3271abb6bf541ef85249b",
            "block_height": 87777
        },
        "mempool_timestamp": null,
        "estimated_blocks": []
    }
}
```

### Rune Info (Batch)

```bash
curl -X POST "http://localhost:8080/runes/info" \
  -H "Content-Type: application/json" \
  -d '["30562:50", "ABCDEF"]' | jq .
```

```json
{
    "data": {
        "30562:50": {
            "id": "30562:50",
            "name": "BESTINSLOTXYZ",
            "spaced_name": "BESTINSLOT•XYZ",
            "symbol": "ʃ",
            "divisibility": 8,
            "etching_tx": "63937d48e35d15a7c5530469210c202104cc94a945cc848554f336b3f4f24121",
            "etching_height": 30562,
            "terms": {
                "amount": "100000000",
                "cap": "3402823669209384634633746074316",
                "start_height": null,
                "end_height": null
            },
            "premine": "100000000"
        },
        "ABCDEF": null
    },
    "indexer_info": {
        "chain_tip": {
            "block_hash": "00000000000000108a4cd9755381003a01bea7998ca2d770fe09b576753ac7ef",
            "block_height": 31633
        },
        "mempool_timestamp": null,
        "estimated_blocks": []
    }
}
```

---

### Mempool-Aware Rune UTXOs by Address

```bash
curl -X GET "http://localhost:8080/addresses/tb1pn9dzakm6egrv90c9gsgs63axvmn6ydwemrpuwljnmz9qdk38ueqsqae936/runes/utxos?mempool=true" | jq .
```

```json
{
    "data": [
        {
            "tx_hash": "63937d48e35d15a7c5530469210c202104cc94a945cc848554f336b3f4f24121",
            "output_index": 1,
            "height": 30562,
            "satoshis": "10000",
            "runes": [
                {
                    "id": "30562:50",
                    "amount": "100000000"
                }
            ]
        }
    ],
    "indexer_info": {
        "chain_tip": {
            "block_hash": "000000002ec229e75c52e8e9adf95149fdde167b59c3271abb6bf541ef85249b",
            "block_height": 87777
        },
        "mempool_timestamp": "2025-06-23 22:04:31",
        "estimated_blocks": [
            {
                "block_height": 87778
            }
        ]
    }
}
```

#### Rune Balance Changes in a Transaction

```bash
curl -X GET "http://localhost:8080/addresses/<ADDRESS>/runes/tx/<TXID>" | jq .
```

```json
{
    "data": [
        {
            "rune_id": "30562:50",
            "amount": "100000000",
            "output": 1,
            "block_height": 30562
        }
    ],
    "indexer_info": {
        "chain_tip": {
            "block_hash": "0000000000000035ec326a15b2f81822962f786028f33205b74b47a9b7cf3caf",
            "block_height": 38980
        },
        "mempool_timestamp": null,
        "estimated_blocks": []
    }
}
```

---

## Contributing

Pull requests and issues are welcome! See the [Kanban board](https://github.com/orgs/maestro-org/projects/16/views/1) for project status and tasks.

---

## License

This project is licensed under the [Apache 2.0 License](./LICENSE).
