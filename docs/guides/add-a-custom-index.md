# How to Add a Custom Index in Maestro Symphony

The [Maestro Symphony](https://github.com/maestro-org/maestro-symphony) is a fast, mempool-aware, and extensible Bitcoin indexer and API server. It provides a framework for indexing UTXOs, metaprotocols, and any other onchain activity.

This guide explains how to construct and integrate custom logic to be extracted and persisted during transaction processing by the Maestro Symphony indexer.

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
pub mod indexer;
pub mod tables;
```

It is recommended to split logic across multiple files (see the [`runes`](../../src/sync/stages/index/indexers/custom/runes/) indexer for reference).

---

### 2. Register a New Enum Variant

Create a new `TransactionIndexerType` enum variant:

```
src/sync/stages/index/indexers/custom/mod.rs
```

Add your variant **only at the end**. Example:

```rust
/// Unique u8 for each transaction indexer, used in the key encodings. Do not modify, only add new
/// variants.
#[derive(Clone, Copy, Encode, Decode, PartialEq, Eq, std::hash::Hash, Debug)]
#[repr(u8)]
pub enum TransactionIndexerType {
    TxCountByAddress = 0,
    Runes = 1,
    UtxosByAddress = 2,
    MyProjIndexer = 3,      // my_proj indexer
}
```

> ⚠️ **Do not reorder or delete existing variants.** They are encoded as `u8` and reused in key encodings. Any change will corrupt existing indexed data unless starting from a clean state.

---

### 3. Define the Indexer Object

Implement a struct that represents your indexer and implements the `ProcessTransaction` trait.

```rust
pub struct MyProjIndexer {
    // arbitrary indexer-specific config options
    start_height: u64,
    track_inputs: bool,
}

impl ProcessTransaction for MyProjIndexer {
    fn process_tx(
        &mut self,
        task: &mut IndexerTask,
        tx: &Transaction,
        ctx: &IndexerContext,
    ) -> Result<(), Error> {
        ...
    }
}
```

Where:

-   `task`: read/write interface to storage
-   `tx`: the transaction being processed
-   `ctx`: context with input resolver, block height, hash, network, arbitrary data to outputs, etc.

A _resolver_ lets you provide a transcation input UTXO reference and receive the corresponding transaction output UTXO.

Reference:
[`runes/indexer.rs#L41-L61`](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/runes/indexer.rs#L41-L61)

---

### 4. Implement and Handle Config

Your indexer should expose a `new()` function that takes a configuration struct and returns an instance of the indexer.

This enables configuration-driven behavior such as start_height, track_inputs, or custom logic.

```rust
impl MyProjIndexer {
    pub fn new(config: MyProjIndexerConfig) -> Result<Self, Error> {
        let start_height = config.start_height;
        let track_inputs = config.track_inputs;

        Ok(Self {
            start_height,
            track_inputs,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct MyProjIndexerConfig {
    #[serde(default)]
    pub start_height: u64,
    #[serde(default)]
    pub track_inputs: bool,
}
```

The `MyProjIndexerConfig` struct should define fields relevant to your indexer.

[Reference](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/runes/indexer.rs#L41-L72)

### 5. Define Storage Tables

Define custom key-value tables for storing the new data using the `define_indexer_table!` macro.

```
src/sync/stages/index/indexers/custom/my_proj/tables.rs
```

Example:

```rust
define_indexer_table! {
    name: MyProjIndexerKV,
    key_type: ScriptPubKey,
    value_type: u64,
    indexer: TransactionIndexer::MyProjIndexer,
    table: 0
}
```

```rust
// key-value:
address => tx_count
```

Each table must:

-   Have a unique `table` ID
-   Use your new enum variant
-   Use key/value types that implement `Encode` and `Decode`

Reference:
[`tx_count_by_address.rs#L20-L26`](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/tx_count_by_address.rs#L20-L26)

---

### 6. Implement `ProcessTransaction`

Process each transaction by:

-   Iterating over inputs, ouputs, resolving UTXOs, etc.
-   Reading/writing to storage with `task.get` and `task.put`

```rust
impl ProcessTransaction for MyProjIndexer {
    fn process_tx(
        &mut self,
        task: &mut IndexerTask,
        tx: &Transaction,
        ctx: &IndexerContext,
    ) -> Result<(), Error> {
        let TransactionWithId { tx, .. } = tx;
        ...
        // retrieve value from KV store
        task.get::<MyProjIndexerKV>(&key)?;
        ...
        // set value in KV store
        task.put::<MyProjIndexerKV>(&key, &value)?;
        ...
    }
}

```

Example:
[`tx_count_by_address.rs#L38-L76`](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/tx_count_by_address.rs#L38-L76)

---

### 7. Register Module and Add to Factory

Add your `my_proj` module in `custom/mod.rs`:

```rust
pub mod id;
pub mod runes;
pub mod tx_count_by_address;
pub mod utxos_by_address;
pub mod my_proj;            // my_proj indexer
```

[Reference](https://github.com/maestro-org/maestro-symphony/blob/main/src/sync/stages/index/indexers/custom/mod.rs#L11C1-L14C26)

Next, add your variant to:

-   The `TransactionIndexerFactory` enum
-   The `create_indexer` function

```rust
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type")]
pub enum TransactionIndexerFactory {
    TxCountByAddress,
    Runes(RunesIndexerConfig),
    UtxosByAddress,
    MyProjIndexer(MyProjIndexerConfig),
}

impl TransactionIndexerFactory {
    pub fn create_indexer(self) -> Result<Box<dyn ProcessTransaction>, Error> {
        match self {
            Self::TxCountByAddress => Ok(Box::new(TxCountByAddressIndexer::new())),
            Self::Runes(c) => Ok(Box::new(RunesIndexer::new(c)?)),
            Self::UtxosByAddress => Ok(Box::new(UtxosByAddressIndexer::new())),
            Self::MyProjIndexer(c) => Ok(Box::new(MyProjIndexer::new(c)?)),
        }
    }
}
```

---

### 8. (Optional) Attach Metadata to UTXOs

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
