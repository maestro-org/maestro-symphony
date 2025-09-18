# Snapshot Guide

This guide explains how to use pre-synced snapshots to quickly set up Maestro Symphony and Bitcoin Core, avoiding the time-consuming initial chain synchronization.

## Overview

Snapshots allow you to bootstrap your Symphony indexer with pre-synchronized data:

-   **Bitcoin node snapshots**: Pre-synced Bitcoin Core blockchain data
-   **Symphony snapshots**: Pre-indexed blockchain data and database state

## Prerequisites

-   [Bitcoin Core (22+)](https://hub.docker.com/r/bitcoin/bitcoin) with RPC and P2P access
-   [lz4](https://github.com/lz4/lz4)
-   Sufficient disk space (see deployment requirements in [README](../../README.md))

## Available Snapshots

**Networks supported:**

-   mainnet
-   testnet4
-   regtest

**Snapshots**

#### Bitcoin

* Mainnet: https://snapshots.gomaestro.org/bitcoin-node/mainnet/snapshots/20250826.tar.lz4
* Testnet: https://snapshots.gomaestro.org/bitcoin-node/testnet/snapshots/20250827.tar.lz4

#### Symphony

* Mainnet: https://snapshots.gomaestro.org/symphony/mainnet/snapshots/20250826.tar.lz4
* Testnet: https://snapshots.gomaestro.org/symphony/testnet/snapshots/20250827.tar.lz4

## Testnet setup example

### 1. Prepare Directories

_The following are to be excuted within maestro-symphony repo directory_.

```bash
mkdir -p ./tmp/{symphony-data,bitcoin-data}
```

### 2. Download Snapshots

Bitcoin node snapshot:

```bash
curl -L https://snapshots.gomaestro.org/bitcoin-node/testnet/snapshots/20250827.tar.lz4 | \
 lz4 -d | tar -xf - -C ./tmp/bitcoin-data
```

Symphony snapshot:

```bash
curl -L https://snapshots.gomaestro.org/symphony/testnet/snapshots/20250827.tar.lz4 | \
  lz4 -d | tar -xf - -C ./tmp/symphony-data
```

### 3. Start Services

```bash
make COMPOSE_FILE=docker-compose.yml compose-up
```

## Verification

Once the services are started, verify the setup:

### Check Symphony Status

```bash
curl http://localhost:8080/addresses/tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx/utxos | jq '.indexer_info'
```

### Stop Services

When you are finished interacting with Symphony, be sure to stop the services as well.

```bash
make compose-down
```

## Troubleshooting

### Disk Space

Ensure sufficient disk space:

-   Testnet: ~5GB for Symphony + ~50GB for Bitcoin node
-   Mainnet: ~100GB for Symphony + ~600GB for Bitcoin node

### Snapshot Age

If snapshots are older than expected:

-   Symphony will automatically sync missing blocks
-   Bitcoin Core will resume from snapshot point
-   Initial startup may take longer for older snapshots

## Updating Snapshots

Snapshots are updated regularly. To use a newer snapshot:

1. Stop Docker services
2. Backup any important data
3. Remove old data directories
4. Download and extract new snapshots
5. Restart Docker services

**Note:** Ensure that no other bitcoin node containers are running as this may cause conflicts.

---

## 🎉 You’re Done!

You have now walked through a guide on how to load both a [Symphony](https://github.com/maestro-org/maestro-symphony) and Bitcoin snapshot.

Be sure to check out [Maestro's additional services](https://docs.gomaestro.org/bitcoin) for further assisting your development of building on Bitcoin.

---

## Support

For issues with snapshots:

-   Open an [issue](https://github.com/maestro-org/maestro-symphony/issues)
-   Join the [Discord community](https://discord.gg/SJgkEje7)
