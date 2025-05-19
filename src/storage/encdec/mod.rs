pub mod decode;
pub mod encode;

pub use decode::{DecodingError, DecodingResult};

pub trait Encode {
    fn encode(&self) -> Vec<u8>;
}

pub trait Decode
where
    Self: Sized,
{
    fn decode(bytes: &[u8]) -> DecodingResult<Self>;

    /// `decode` but ignoring, and not returning, any remaining bytes
    fn decode_all(bytes: &[u8]) -> Result<Self, DecodingError> {
        Self::decode(bytes).map(|x| x.0)
    }
}

#[derive(Default, Clone)]
pub struct EncodeBuilder {
    output: Vec<u8>,
}

impl EncodeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append<T: Encode>(mut self, data: &T) -> Self {
        self.output.extend(data.encode());
        self
    }

    pub fn build(self) -> Vec<u8> {
        self.output
    }
}

/// Unsigned integer with more efficient serialisation while maintaining lexicographic ordering
#[derive(Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
struct VarUInt(pub u128);

macro_rules! impl_to_varuint {
    ($type:ty) => {
        impl From<$type> for VarUInt {
            fn from(val: $type) -> Self {
                VarUInt(val.try_into().unwrap())
            }
        }
    };
}

impl_to_varuint!(usize);
impl_to_varuint!(u8);
impl_to_varuint!(u16);
impl_to_varuint!(u32);
impl_to_varuint!(u64);
impl_to_varuint!(u128);

macro_rules! impl_try_from_varuint {
    ($type:ty) => {
        impl TryFrom<VarUInt> for $type {
            type Error = DecodingError;

            fn try_from(val: VarUInt) -> Result<$type, Self::Error> {
                let inner_val = val.0;
                inner_val
                    .try_into()
                    .map_err(|_| DecodingError::VarUIntCasting(inner_val))
            }
        }
    };
}

impl_try_from_varuint!(usize);
impl_try_from_varuint!(u8);
impl_try_from_varuint!(u16);
impl_try_from_varuint!(u32);
impl_try_from_varuint!(u64);
impl_try_from_varuint!(u128);
