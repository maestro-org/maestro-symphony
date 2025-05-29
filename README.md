_from hash to harmony: an orchestrated Bitcoin indexing suite_

# The Maestro Symphony

## Local deployment

Run a node and point to it in the run [config](examples/testnet.toml).

Sync:

```
RUST_LOG=info cargo run -- examples/testnet.toml run
```

To query you must first stop syncing, but this will change.

Query Rune etching information:

```
RUST_LOG=info cargo run -- query 30562:50

rune info for 30562:50:
RuneInfo { name: 212002398557419859, terms: Some(RuneTerms { amount: Some(100000000), cap: Some(3402823669209384634633746074316), start_height: None, end_height: None }), symbol: Some(643), divisibility: 8, etching_height: 30562, etching_tx: [33, 65, 242, 244, 179, 54, 243, 84, 133, 132, 204, 69, 169, 148, 204, 4, 33, 32, 12, 33, 105, 4, 83, 197, 167, 21, 93, 227, 72, 125, 147, 99], premine: 100000000, spacers: 512 }
```

Query address rune UTxOs (divisibility not accounted for in rune amounts):

```
RUST_LOG=info cargo run -- query tb1pn9dzakm6egrv90c9gsgs63axvmn6ydwemrpuwljnmz9qdk38ueqsqae936

utxos containing runes controlled by tb1pn9dzakm6egrv90c9gsgs63axvmn6ydwemrpuwljnmz9qdk38ueqsqae936 (divisibility ignored):
>> 63937d48e35d15a7c5530469210c202104cc94a945cc848554f336b3f4f24121#1 -> 10000 sats + [(RuneId { block: 30562, tx: 50 }, 100000000)]
```

# TODOs

-   [x] Base RocksDB database handler
-   [x] Base indexer
-   [x] Base UTXO resolution
-   [x] Node communications handler
-   [ ] Rollback buffer and processing
-   [ ] Mempool processing with p2p
-   [ ] Custom indexer: runes
-   [ ] Customer endpoint: runes by address
