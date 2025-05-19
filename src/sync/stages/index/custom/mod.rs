use maestro_symphony_macros::{Decode, Encode};

// mod runes;
// mod tx_count_by_address;

#[derive(Encode, Decode, PartialEq, Eq, std::hash::Hash)]
#[repr(u8)]
pub enum CustomIndexer {
    TxCountByAddress,
    Runes,
}
