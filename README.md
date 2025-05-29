_from hash to harmony: an orchestrated Bitcoin indexing suite_

# The Maestro Symphony

Port-forward to node:
```
kubectl -n bitcoin-testnet-dataplane-0 port-forward svc/bitcoin-node 8333:8333
```

Sync:
```
RUST_LOG=info cargo run -- examples/testnet.toml run
```

To query you must first stop syncing, but this will change.

Query Rune etching information:
```
RUST_LOG=info cargo run -- query 30562:50
```

Query address rune UTxOs (divisibility not accounted for in rune amounts):
```
RUST_LOG=info cargo run -- query tb1pn9dzakm6egrv90c9gsgs63axvmn6ydwemrpuwljnmz9qdk38ueqsqae936
```