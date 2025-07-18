use crate::{
    DecodingResult,
    storage::encdec::{Decode, Encode, decode::malformed_input},
};

/// Operation types for merge operations
#[derive(Debug, Clone, PartialEq)]
#[repr(u8)]
pub enum MergeOperation {
    Increment(u128) = 0,
    Decrement(u128) = 1,
}

impl Encode for MergeOperation {
    fn encode(&self) -> Vec<u8> {
        match self {
            MergeOperation::Increment(amount) => [vec![0], amount.encode()].concat(),
            MergeOperation::Decrement(amount) => [vec![1], amount.encode()].concat(),
        }
    }
}

impl Decode for MergeOperation {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        let (op_code, bytes) = u8::decode(bytes)?;

        match op_code {
            0 => {
                let (amount, bytes) = u128::decode(bytes)?;
                Ok((MergeOperation::Increment(amount), bytes))
            }
            1 => {
                let (amount, bytes) = u128::decode(bytes)?;
                Ok((MergeOperation::Decrement(amount), bytes))
            }
            _ => Err(malformed_input("invalid merge op code", bytes)),
        }
    }
}

/// Creates a merge operator for RocksDB that handles increment and decrement operations
pub fn create_merge_operator()
-> impl Fn(&[u8], Option<&[u8]>, &rocksdb::MergeOperands) -> Option<Vec<u8>> {
    |_key, existing, operands| {
        // Start with existing value or 0 (change if we need something other than u128)
        let mut current = match existing {
            Some(b) => u128::decode_all(b).ok()?, // return None for error
            None => 0,
        };

        // Process each operand in sequence
        for operand in operands.iter() {
            let merge_op = MergeOperation::decode_all(operand).ok()?;

            match merge_op {
                MergeOperation::Increment(delta) => current = current.saturating_add(delta),
                MergeOperation::Decrement(delta) => current = current.saturating_sub(delta),
            }
        }

        // Encode the result
        Some(current.encode())
    }
}

/// Creates a partial merge operator for RocksDB that combines multiple operands into a net operation
pub fn create_partial_merge_operator()
-> impl Fn(&[u8], Option<&[u8]>, &rocksdb::MergeOperands) -> Option<Vec<u8>> {
    |_key, _existing, operands| {
        // Track net changes for increment and decrement operations
        let mut increment_total: u128 = 0;
        let mut decrement_total: u128 = 0;

        // Process all operands to calculate the net change
        for operand in operands.iter() {
            let merge_op = MergeOperation::decode_all(operand).ok()?;

            match merge_op {
                MergeOperation::Increment(delta) => {
                    increment_total = increment_total.saturating_add(delta)
                }
                MergeOperation::Decrement(delta) => {
                    decrement_total = decrement_total.saturating_add(delta)
                }
            }
        }

        // Calculate the net change
        let new_op = if increment_total >= decrement_total {
            // Net increment
            MergeOperation::Increment(increment_total.saturating_sub(decrement_total))
        } else {
            // Net decrement
            MergeOperation::Decrement(decrement_total.saturating_sub(increment_total))
        };

        Some(new_op.encode())
    }
}

/// Applies a merge operation to an existing value
pub fn apply_merge_to_value(
    op: &MergeOperation,
    existing_value: &[u8],
) -> Result<u128, crate::error::Error> {
    // Decode the existing value
    let existing_amount = u128::decode_all(existing_value)?;

    // Apply the operation to the existing value
    let result = match op {
        MergeOperation::Increment(amount) => existing_amount.saturating_add(*amount),
        MergeOperation::Decrement(amount) => existing_amount.saturating_sub(*amount),
    };

    Ok(result)
}

/// Combines two merge operations into a single operation
pub fn combine_merge_operations(
    existing_op: &MergeOperation,
    new_op: &MergeOperation,
) -> MergeOperation {
    match (existing_op, new_op) {
        (MergeOperation::Increment(existing_amount), MergeOperation::Increment(new_amount)) => {
            MergeOperation::Increment(existing_amount.saturating_add(*new_amount))
        }
        (MergeOperation::Decrement(existing_amount), MergeOperation::Decrement(new_amount)) => {
            MergeOperation::Decrement(existing_amount.saturating_add(*new_amount))
        }
        (MergeOperation::Increment(existing_amount), MergeOperation::Decrement(new_amount)) => {
            if *new_amount <= *existing_amount {
                MergeOperation::Increment(existing_amount.saturating_sub(*new_amount))
            } else {
                MergeOperation::Decrement(new_amount.saturating_sub(*existing_amount))
            }
        }
        (MergeOperation::Decrement(existing_amount), MergeOperation::Increment(new_amount)) => {
            if *new_amount <= *existing_amount {
                MergeOperation::Decrement(existing_amount.saturating_sub(*new_amount))
            } else {
                MergeOperation::Increment(new_amount.saturating_sub(*existing_amount))
            }
        }
    }
}
