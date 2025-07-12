# How to Add a Custom Index in Maestro Symphony

The [Maestro Symphony](https://github.com/maestro-org/maestro-symphony) is a fast, mempool-aware, and extensible Bitcoin indexer and API server. It provides a framework for indexing UTXOs, metaprotocols, and any other onchain activity.

This guide explains how to construct and integrate custom logic to be extracted and persisted during transaction processing by the Maestro Symphony indexer.

## ✅ Summary

1. Create a module under `custom/`
2. Add enum variant to `TransactionIndexerType`
3. Implement the `ProcessTransaction` trait
4. Define your indexer's storage tables
5. Write transaction processing logic
6. Register your indexer in the factory
7. (Optional) Attach metadata to UTXOs

## Steps

### 1. Create a New Indexer Project and Initialize it

After [forking](https://github.com/maestro-org/maestro-symphony/fork) the repo, create a file or module in the [`/custom`](../../src/sync/stages/index/indexers/custom/) subdirectory.

_Setup a new project directory_

```bash
mkdir -p src/sync/stages/index/indexers/custom/my_proj
```

_Create the files (plus any extra that you need)_

```bash
touch mod.rs indexer.rs tables.rs
```

_Generate the `mod.rs`_ (and add your related files)

```bash
cat>>mod.rs
pub mod indexer;
pub mod tables;
```

It is recommended to split logic across multiple files (see the [`runes`](../../src/sync/stages/index/indexers/custom/runes/) indexer for reference).

---

### 2. Register a New Enum Variant

Edit the `TransactionIndexerType` enum:

```

src/sync/stages/index/indexers/custom/mod.rs

```

Add your variant **only at the end**. Example:

```rust
pub enum TransactionIndexerType {
    Runes = 0,
    TxCountByAddress = 1,
    YourIndexer = 2,
}
```

> ⚠️ **Do not reorder or delete existing variants.** They are encoded as `u8` and reused in key encodings. Any change will corrupt existing indexed data unless starting from a clean state.

---

### 3. Define the Indexer Object

Implement a struct that represents your indexer and implements the `ProcessTransaction` trait.

Core function:

```rust
fn process_tx(&mut self, task: &mut IndexerTask, tx: &Transaction, ctx: &IndexerContext)
```

Where:

-   `task`: read/write interface to storage
-   `tx`: the transaction being processed
-   `ctx`: context with input resolver, block height, hash, network, etc.

Reference:
[`runes/indexer.rs#L41-L61`](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/runes/indexer.rs#L41-L61)

---

### 4. Define Storage Tables

Define custom key-value tables for storing your data.

Example:
[`tx_count_by_address.rs#L20-L26`](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/tx_count_by_address.rs#L20-L26)

Each table must:

-   Have a unique `table` ID
-   Use your enum variant
-   Use key/value types that implement `Encode` and `Decode`

---

### 5. Implement `ProcessTransaction`

Process each transaction by:

-   Iterating over inputs and resolving UTXOs (skip coinbase inputs)
-   Iterating over outputs
-   Tracking affected addresses or metadata
-   Reading/writing to storage with:

```rust
task.get::<YourTable>(&key)?;
task.put::<YourTable>(&key, &value)?;
```

Example:
[`tx_count_by_address.rs#L38-L76`](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/tx_count_by_address.rs#L38-L76)

---

### 6. Register in the Factory

In `custom/mod.rs`, add your variant to:

-   The `TransactionIndexerFactory` enum
-   The `create_indexer` function

Example config:

```toml
[[indexers]]
type = "YourIndexer"
start_height = 840000
index_activity = true
```

---

### 7. (Optional) Attach Metadata to UTXOs

To persist data across transactions using UTXOs (e.g., inscriptions, runes), you can attach metadata during output processing:

```rust
task.attach_metadata_to_output(vout, &data)?;
```

Later, during input resolution:

```rust
let meta = ctx.resolver.resolve_input(input)?.metadata::<YourType>()?;
```

Examples:

-   [Attach metadata](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/runes/indexer.rs#L256-L260)
-   [Retrieve metadata](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/runes/indexer.rs#L318-L321)

_This saves storage and avoids manually tracking UTXOs elsewhere._

---

For working examples, refer to:

-   [`runes`](https://github.com/maestro-org/maestro-symphony/tree/main/src/sync/stages/index/indexers/custom/runes)
-   [`tx_count_by_address`](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/tx_count_by_address.rs)

## 🎉 You’re Done!

You have now walked through a guide on how to index a custom piece of data and add an API endpoint using the [Maestro Symphony](https://github.com/maestro-org/maestro-symphony).

Be sure to check out [Maestro's additional services](https://www.gomaestro.org/chains/bitcoin) for further assisting your development of building on Bitcoin.

---

### Support

If you are experiencing any trouble with the above, reach out on <a href="https://discord.gg/ES2rDhBJt3" target="_blank">Discord</a>.
