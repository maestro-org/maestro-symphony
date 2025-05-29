use std::str::FromStr;

use super::id::ProcessTransaction;
use crate::error::Error;
use crate::storage::kv_store::Task;
use crate::sync::stages::TransactionWithId;
use crate::sync::stages::index::worker::context::IndexingContext;
use bitcoin::Address;
use serde::Deserialize;

pub struct TxCountByAddressIndexer {
    addresses: Vec<Address>,
}

impl TxCountByAddressIndexer {
    pub fn new(config: TxCountByAddressConfig) -> Result<Self, Error> {
        let addresses = config
            .addresses
            .into_iter()
            .map(|x| Address::from_str(&x).map(|x| x.assume_checked()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap(); // TODO error handling

        Ok(Self { addresses })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TxCountByAddressConfig {
    #[serde(default)]
    pub addresses: Vec<String>,
}

impl ProcessTransaction for TxCountByAddressIndexer {
    fn process_tx(
        &self,
        _task: &mut Task,
        tx: &TransactionWithId,
        _tx_block_index: usize,
        ctx: &mut IndexingContext,
    ) -> Result<(), Error> {
        let TransactionWithId { tx, .. } = tx;

        if !tx.is_coinbase() {
            for input in tx.input.iter() {
                ctx.resolve_input(&(input.previous_output.into())).unwrap();

                // ...
            }
        }

        Ok(())
    }
}
