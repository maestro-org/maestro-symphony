use indexmap::IndexMap;

use super::{Encode, VarUInt};

impl<const N: usize> Encode for [u8; N] {
    fn encode(&self) -> Vec<u8> {
        self.to_vec()
    }
}

impl Encode for u8 {
    fn encode(&self) -> Vec<u8> {
        vec![*self]
    }
}

impl Encode for VarUInt {
    fn encode(&self) -> Vec<u8> {
        let bend = self.0.to_be_bytes();

        for idx in 0..16 {
            if bend[idx] != 0x00 {
                let size = 16 - idx;
                let mut out = Vec::with_capacity(1 + size);

                out.push(size.try_into().unwrap());
                out.extend_from_slice(&bend[idx..]);

                return out;
            }
        }

        vec![0]
    }
}

macro_rules! impl_uint_encode {
    ($type:ty) => {
        impl Encode for $type {
            fn encode(&self) -> Vec<u8> {
                Into::<VarUInt>::into(*self).encode()
            }
        }
    };
}

// u8 encoding is more efficient than if we used our varuint
impl_uint_encode!(usize);
impl_uint_encode!(u16);
impl_uint_encode!(u32);
impl_uint_encode!(u64);
impl_uint_encode!(u128);

impl<A: Encode> Encode for Vec<A> {
    fn encode(&self) -> Vec<u8> {
        [
            self.len().encode(),
            self.iter().flat_map(|t| t.encode()).collect(),
        ]
        .concat()
    }
}

impl<K: Encode, V: Encode> Encode for IndexMap<K, V> {
    fn encode(&self) -> Vec<u8> {
        [
            self.len().encode(),
            self.iter()
                .flat_map(|(k, v)| [k.encode(), v.encode()].concat())
                .collect(),
        ]
        .concat()
    }
}
