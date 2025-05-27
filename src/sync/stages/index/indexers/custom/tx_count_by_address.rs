use std::str::FromStr;

use bitcoin::Address;
use serde::Deserialize;

use super::id::ProcessTransaction;
use crate::error::Error;
use crate::storage::kv_store::Task;
use crate::sync::stages::index::worker::context::IndexingContext;
use crate::sync::stages::{Point, TransactionWithId};

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
    pub addresses: Vec<String>,
}

impl ProcessTransaction for TxCountByAddressIndexer {
    fn process_tx(
        &self,
        task: &mut Task,
        tx: &TransactionWithId,
        _tx_block_index: usize,
        ctx: &IndexingContext,
    ) -> Result<(), Error> {
        let TransactionWithId { tx, .. } = tx;

        for input in tx
            .input
            .iter()
            .flat_map(|txin| ctx.resolve_input(&(txin.previous_output.into())))
        {}

        Ok(())
    }
}
