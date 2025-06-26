# Maestro Symphony

_from hash to harmony: an orchestrated Bitcoin indexing suite_

---

## Overview

Maestro Symphony is a fast, mempool-aware Bitcoin indexer and API server. It supports mainnet and testnet4, providing data for UTXOs, metaprotocols and much more. Designed for developers who need reliable, mempool-aware blockchain data.

---

## Features

-   **Supported Networks:** mainnet, testnet4
-   **Indexers:**
    -   Runes
    -   Transaction count by address
    -   UTXOs by address
-   **Mempool Awareness:** Query with `?mempool=true` to include unconfirmed transactions.

---

## Quick Start

### Prerequisites

-   A running Bitcoin node (mainnet or testnet4)
-   Set the `node_address` parameter in your config (see `examples/testnet.toml`)
-   Rust toolchain installed

### Run All (Sync + Serve)

```bash
RUST_LOG=info cargo run -- examples/testnet.toml run
```

### Sync Only

```bash
RUST_LOG=info cargo run -- examples/testnet.toml sync
```

### Serve Only

```bash
RUST_LOG=info cargo run -- examples/testnet.toml serve
```

---

## Configuration

Edit the config file (e.g., `examples/testnet.toml`) to set your node address and other parameters.

---

## API Endpoints & Examples

### Addresses

-   **UTXOs by address:**
    -   `GET /addresses/{address}/utxos`
    - Requires indexers: `UtxosByAddress`
-   **Rune UTXOs by address:**
    -   `GET /addresses/{address}/runes/utxos`
    - Requires indexers: `Runes`
-   **Rune UTXOs by address and rune:**
    -   `GET /addresses/{address}/runes/utxos/{rune}`
    - Requires indexers: `Runes`
-   **Rune balances by address:**
    -   `GET /addresses/{address}/runes/balance`
    - Requires indexers: `Runes`
-   **Rune balances by address and rune:**
    -   `GET /addresses/{address}/runes/balances/{rune}`
    - Requires indexers: `Runes`

### Runes

-   **Rune info:**
    -   `GET /runes/{rune}`
    - Requires indexers: `Runes`

#### Example: Rune UTXOs by Address

```bash
curl -X GET http://localhost:8080/addresses/tb1pn9dzakm6egrv90c9gsgs63axvmn6ydwemrpuwljnmz9qdk38ueqsqae936/runes/utxos | jq .
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
          "name": "BESTINSLOTXYZ",
          "spaced_name": "BESTINSLOT•XYZ",
          "quantity": "1.00000000"
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

### Runes

-   **Rune info:**
    -   `GET /runes/{rune}`

#### Example: Rune Info

```bash
curl -X GET http://localhost:8080/runes/30562:50 | jq .
```

```json
{
  "data": {
    "id": "30562:50",
    "name": "BESTINSLOTXYZ",
    "spaced_name": "BESTINSLOT•XYZ",
    "symbol": "ʃ",
    "divisibility": 8,
    "etching_tx": "63937d48e35d15a7c5530469210c202104cc94a945cc848554f336b3f4f24121",
    "etching_height": 30562,
    "terms": {
      "amount": "1.00000000",
      "cap": "34028236692093846346337.46074316",
      "start_height": null,
      "end_height": null
    },
    "premine": "1.00000000"
  },
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

---

### Mempool awareness

#### Example: Rune UTXOs by Address

```bash
curl -X GET http://localhost:8080/addresses/tb1pn9dzakm6egrv90c9gsgs63axvmn6ydwemrpuwljnmz9qdk38ueqsqae936/runes/utxos?mempool=true | jq .
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
          "name": "BESTINSLOTXYZ",
          "spaced_name": "BESTINSLOT•XYZ",
          "quantity": "1.00000000"
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

#### Example: Rune Balance Changes in a Transaction

```bash
curl -X GET http://localhost:8080/addresses/<ADDRESS>/runes/tx/<TXID> | jq .
```

```json
{
  "data": [
    {
      "rune_id": "30562:50",
      "amount": "1.00000000",
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

## Local Deployment Requirements

### Testnet4

-   Disk: 1 GB
-   CPU: 2 cores
-   RAM: 4 GB
-   Approximate sync time: 2 hours

---

## Contributing

Pull requests and issues are welcome! See the [Kanban board](https://github.com/orgs/maestro-org/projects/16/views/1) for project status and tasks.

---

## License

This project is licensed under the terms of the [Apache 2.0 License](./LICENSE).
