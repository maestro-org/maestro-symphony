# Snapshot Guide

This guide explains how to use pre-synced snapshots to quickly set up Maestro Symphony and Bitcoin Core, avoiding the time-consuming initial chain synchronization.

## Overview

Snapshots allow you to bootstrap your Symphony indexer with pre-synchronized data:

- **Symphony snapshots**: Pre-indexed blockchain data and database state
- **Bitcoin node snapshots**: Pre-synced Bitcoin Core blockchain data

## Prerequisites

- `lz4` compression utility
- `curl` for downloading
- Sufficient disk space (see deployment requirements in [README](../../README.md))

### Install lz4

**macOS:**
```bash
brew install lz4
```

**Ubuntu/Debian:**
```bash
sudo apt-get install lz4
```

## Available Snapshots

**Networks supported:**
- testnet4
- mainnet (coming soon)

**Snapshot locations:**
- Symphony: https://dash.cloudflare.com/c4e0407294a743505c3e8823451b1fb1/r2/default/buckets/maestro-org-public-snapshots?prefix=symphony%2F
- Bitcoin node: https://dash.cloudflare.com/c4e0407294a743505c3e8823451b1fb1/r2/default/buckets/maestro-org-public-snapshots?prefix=bitcoin-node%2F

## Setup Steps

### 1. Prepare Directories

```bash
mkdir -p ~/workspace/{symphony-data,bitcoin-data}
```

### 2. Download Both Snapshots

```bash
# Download Symphony snapshot
curl -L https://dash.cloudflare.com/c4e0407294a743505c3e8823451b1fb1/r2/default/buckets/maestro-org-public-snapshots?prefix=symphony%2F | \
  lz4 -d | tar -xf - -C ~/workspace/symphony-data

# Download Bitcoin node snapshot  
curl -L https://dash.cloudflare.com/c4e0407294a743505c3e8823451b1fb1/r2/default/buckets/maestro-org-public-snapshots?prefix=bitcoin-node%2F | \
  lz4 -d | tar -xf - -C ~/workspace/bitcoin-data
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
sudo chown -R $USER:$USER ~/workspace/symphony-data
sudo chown -R $USER:$USER ~/workspace/bitcoin-data
```

### Disk Space

Ensure sufficient disk space:
- Testnet: ~1GB for Symphony + ~50GB for Bitcoin node
- Mainnet: ~24GB for Symphony + ~600GB for Bitcoin node

### Snapshot Age

If snapshots are older than expected:
- Symphony will automatically sync missing blocks
- Bitcoin Core will resume from snapshot point
- Initial startup may take longer for older snapshots

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

Be sure to check out [Maestro's additional services](https://www.gomaestro.org/chains/bitcoin) for further assisting your development of building on Bitcoin.

---

## Support

For issues with snapshots:
- Check [troubleshooting section](../README.md#troubleshooting) in README
- Report issues on [GitHub](https://github.com/maestro-org/maestro-symphony/issues)
- Join the [Discord community](https://discord.gg/SJgkEje7)

