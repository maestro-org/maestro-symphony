# Snapshot Guide

This guide explains how to use pre-synced snapshots to quickly set up Maestro Symphony and Bitcoin Core, avoiding the time-consuming initial chain synchronization.

## Overview

Snapshots allow you to bootstrap your Symphony indexer with pre-synchronized data:

-   **Bitcoin node snapshots**: Pre-synced Bitcoin Core blockchain data
-   **Symphony snapshots**: Pre-indexed blockchain data and database state

## Prerequisites

-   [Bitcoin Core (22+)](https://hub.docker.com/r/bitcoin/bitcoin) with RPC and P2P access
-   Sufficient disk space (see deployment requirements in [README](../../README.md))

## Available Snapshots

**Networks supported:**

-   mainnet
-   testnet4
-   regtest

**Snapshots**

#### Bitcoin

Mainnet: https://snapshots.gomaestro.org/bitcoin-node/mainnet/snapshots/20250826.tar.lz4
Testnet: https://snapshots.gomaestro.org/bitcoin-node/testnet/snapshots/20250827.tar.lz4

#### Symphony

Mainnet: https://snapshots.gomaestro.org/symphony/mainnet/snapshots/20250826.tar.lz4
Testnet: https://snapshots.gomaestro.org/symphony/testnet/snapshots/20250827.tar.lz4

## Setup Steps

### 1. Prepare Directories

```bash
mkdir -p ~/workspace/{symphony-data,bitcoin-data}
```

### 2. Download Both Snapshots

# Download Bitcoin node snapshot

```bash
curl -L https://snapshots.gomaestro.org/bitcoin-node/testnet/snapshots/20250827.tar.lz4 | \
 lz4 -d | tar -xf - -C ~/workspace/bitcoin-data
```

```bash
# Download Symphony snapshot
curl -L https://snapshots.gomaestro.org/symphony/testnet/snapshots/20250827.tar.lz4 | \
  lz4 -d | tar -xf - -C ~/workspace/symphony-data
```

### 3. Start Bitcoin Core

```bash
bitcoind -testnet -datadir=~/workspace/bitcoin-data -server -rpcuser=bitcoin -rpcpassword=password
```

### 4. Configure Symphony

Create `snapshot-testnet.toml`:

```toml
db_path = "~/workspace/symphony-data"

[sync.node]
p2p_address = "localhost:18333"
rpc_address = "http://localhost:18332"
rpc_user = "bitcoin"
rpc_pass = "password"

[sync]
network = "testnet4"
safe_mode = true
mempool = true

[sync.indexers]
transaction_indexers = [
    { name = "runes" },
    { name = "utxos_by_address" },
    { name = "tx_count_by_address" }
]

[server]
address = "0.0.0.0:8080"
```

### 5. Start Symphony

```bash
make run CONFIG=snapshot-testnet.toml
```

## Verification

Once started, verify the setup:

### Check Symphony Status

```bash
curl http://localhost:8080/addresses/tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx/utxos | jq '.indexer_info'
```

### Check Bitcoin Core

```bash
bitcoin-cli -testnet getblockchaininfo
```

## Troubleshooting

### Permission Issues

If you encounter permission errors:

```bash
# Fix ownership
sudo chown -R $USER:$USER ~/workspace/bitcoin-data
sudo chown -R $USER:$USER ~/workspace/symphony-data
```

### Disk Space

Ensure sufficient disk space:

-   Testnet: ~1GB for Symphony + ~50GB for Bitcoin node
-   Mainnet: ~24GB for Symphony + ~600GB for Bitcoin node

### Snapshot Age

If snapshots are older than expected:

-   Symphony will automatically sync missing blocks
-   Bitcoin Core will resume from snapshot point
-   Initial startup may take longer for older snapshots

## Updating Snapshots

Snapshots are updated regularly. To use a newer snapshot:

1. Stop Symphony and Bitcoin Core
2. Backup any important data
3. Remove old data directories
4. Download and extract new snapshots
5. Restart services

---

## 🎉 You’re Done!

You have now walked through a guide on how to load both a [Symphony](https://github.com/maestro-org/maestro-symphony) and Bitcoin snapshot.

Be sure to check out [Maestro's additional services](https://docs.gomaestro.org/bitcoin) for further assisting your development of building on Bitcoin.

---

## Support

For issues with snapshots:

-   Open an [issue](https://github.com/maestro-org/maestro-symphony/issues)
-   Join the [Discord community](https://discord.gg/SJgkEje7)
