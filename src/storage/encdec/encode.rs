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

impl Encode for u16 {
    fn encode(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
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

macro_rules! impl_varuint_encode {
    ($type:ty) => {
        impl Encode for $type {
            fn encode(&self) -> Vec<u8> {
                Into::<VarUInt>::into(*self).encode()
            }
        }
    };
}

// we wont use varuint for u8 or u16
impl_varuint_encode!(usize);
impl_varuint_encode!(u32);
impl_varuint_encode!(u64);
impl_varuint_encode!(u128);

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

impl Encode for () {
    fn encode(&self) -> Vec<u8> {
        vec![]
    }
}

impl Encode for bool {
    fn encode(&self) -> Vec<u8> {
        vec![*self as u8]
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self) -> Vec<u8> {
        [
            self.is_some().encode(),
            self.as_ref().map(|t| t.encode()).unwrap_or_default(),
        ]
        .concat()
    }
}

impl<A: Encode, B: Encode> Encode for (A, B) {
    fn encode(&self) -> Vec<u8> {
        [self.0.encode(), self.1.encode()].concat()
    }
}
