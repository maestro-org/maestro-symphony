_from hash to harmony: an orchestrated Bitcoin indexing suite_

# The Maestro Symphony

Kanban board: https://github.com/orgs/maestro-org/projects/16/views/1

#### Supported networks

-   mainnet
-   testnet4

#### Indexers

-   runes
-   tx_count_by_address
-   utxos_by_address

#### Endpoints

-   Addresses
    -   UTXOs by address: `/addresses/{address}/utxos`
    -   Rune UTXOs by address: `/addresses/{address}/runes/utxos`
    -   Rune UTXOs by address and rune: `/addresses/{address}/runes/utxos/{rune}`
    -   Rune balances by address: `/addresses/{address}/runes/balance`
    -   Rune balances by address and rune: `/addresses/{address}/runes/balances/{rune}`
-   Runes
    -   Rune info: `/runes/{rune}`

## Deployment

### Prequisites

-   Run a node and point to it via the `node_address` parameter in the run [config](examples/testnet.toml)

### Run: Sync + Serve

```bash
RUST_LOG=info cargo run -- examples/testnet.toml run
```

### Sync only

```bash
RUST_LOG=info cargo run -- examples/testnet.toml sync
```

### Serve only

```bash
RUST_LOG=info cargo run -- examples/testnet.toml serve
```

### Examples

Rune UTXOs by address:

```bash
curl -X GET http://localhost:8080/addresses/tb1pn9dzakm6egrv90c9gsgs63axvmn6ydwemrpuwljnmz9qdk38ueqsqae936/runes/utxos | jq .
[
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
]
```

Rune info

```bash
curl -X GET http://localhost:8080/runes/30562:50 | jq .
{
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
}
```

### Reousrce Requirements

#### Testnet4

-   Disk: 1 GB
-   CPU: 2 cores
-   RAM: 4 GB
-   Approximate sync time: 2 hours
