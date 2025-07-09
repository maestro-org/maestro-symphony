use super::{Decode, VarUInt};

use indexmap::IndexMap;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum DecodingError {
    #[error("Malformed input: {0} ({1:?})")]
    MalformedInput(String, Vec<u8>),
    #[error("Invalid UTF-8: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("VarUInt casting: {0}")]
    VarUIntCasting(u128),
    #[error("Enum kind: {0:?}")]
    InvalidEnumKind(Vec<u8>),
}

// Helper method to create MalformedInput error with just a message
pub fn malformed_input<S: Into<String>>(msg: S, bytes: &[u8]) -> DecodingError {
    DecodingError::MalformedInput(msg.into(), bytes.to_vec())
}

pub type DecodingResult<'a, T> = Result<(T, &'a [u8]), DecodingError>;

impl<const N: usize> Decode for [u8; N] {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        bytes
            .get(..N)
            .map(|slice| {
                (
                    slice.try_into().expect("slice with incorrect length"),
                    &bytes[N..],
                )
            })
            .ok_or(malformed_input("array insufficient bytes", bytes))
    }
}

impl Decode for u8 {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        bytes
            .first()
            .map(|b| (*b, &bytes[1..]))
            .ok_or(malformed_input("u8 insufficient bytes", bytes))
    }
}

impl Decode for u16 {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        <[u8; 2]>::decode(bytes).map(|(b, rest)| (u16::from_be_bytes(b), rest))
    }
}

impl Decode for VarUInt {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        let len = *bytes
            .first()
            .ok_or(malformed_input("varuint insufficient bytes", bytes))?
            as usize;

        if len > 16 {
            return Err(malformed_input("varuint len exceeds maximum", bytes));
        }

        let (data, bytes) = bytes[1..]
            .split_at_checked(len)
            .ok_or(malformed_input("varuint insufficient bytes", bytes))?;

        let be_128: [u8; 16] = [vec![0; 16 - len], data.to_vec()]
            .concat()
            .try_into()
            .unwrap();

        Ok((VarUInt(u128::from_be_bytes(be_128)), bytes))
    }
}

macro_rules! impl_varuint_decode {
    ($t:ty) => {
        impl Decode for $t {
            fn decode(bytes: &[u8]) -> DecodingResult<$t> {
                let (varuint, rem) = VarUInt::decode(bytes)?;

                let casted = Self::try_from(varuint)?;

                Ok((casted, rem))
            }
        }
    };
}

impl_varuint_decode!(usize);
impl_varuint_decode!(u32);
impl_varuint_decode!(u64);
impl_varuint_decode!(u128);

impl<A: Decode> Decode for Vec<A> {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        let (len, mut bytes) = usize::decode(bytes)?;
        let mut vec = Vec::with_capacity(len);

        for _ in 0..len {
            let (item, rest) = A::decode(bytes)?;
            bytes = rest;

            vec.push(item);
        }

        Ok((vec, bytes))
    }
}

impl<K, V> Decode for IndexMap<K, V>
where
    K: Decode + Eq + std::hash::Hash,
    V: Decode + Eq + std::hash::Hash,
{
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        let mut map = IndexMap::new();

        let (len, mut bytes) = usize::decode(bytes)?;

        for _ in 0..len {
            let (key, rest) = K::decode(bytes)?;
            bytes = rest;
            let (value, rest) = V::decode(bytes)?;
            bytes = rest;
            map.insert(key, value);
        }

        Ok((map, bytes))
    }
}

impl Decode for () {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        Ok(((), bytes))
    }
}

impl Decode for bool {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        let (byte, rem) = u8::decode(bytes)?;

        match byte {
            0 => Ok((false, rem)),
            1 => Ok((true, rem)),
            _ => Err(malformed_input("invalid bool byte", bytes)),
        }
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        let (presence, bytes) = bool::decode(bytes)?;

        if !presence {
            return Ok((None, bytes));
        }

        let (value, bytes) = T::decode(bytes)?;
        Ok((Some(value), bytes))
    }
}

impl<A: Decode, B: Decode> Decode for (A, B) {
    fn decode(bytes: &[u8]) -> DecodingResult<Self> {
        let (first, bytes) = A::decode(bytes)?;
        let (second, bytes) = B::decode(bytes)?;

        Ok(((first, second), bytes))
    }
}
