use std::convert::TryFrom;
use std::io;

use serde::{de::DeserializeOwned, Serialize};

pub type EncodeResult = Result<Vec<u8>, io::Error>;
pub type DecodeResult<T> = Result<T, io::Error>;

// encode a type to byte array
pub trait Encode {
    fn encode(&self) -> EncodeResult;
}

// decode from byte array
pub trait Decode: Sized {
    fn decode(data: &[u8]) -> DecodeResult<Self>;
}

impl<T: Serialize> Encode for T {
    fn encode(&self) -> EncodeResult {
        bincode::serialize(&self)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Failed to serialize"))
    }
}

impl<T> Decode for T
where
    T: DeserializeOwned,
{
    fn decode(data: &[u8]) -> DecodeResult<Self> {
        bincode::deserialize(data)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Failed to deserialize"))
    }
}

pub fn to_base<T: ?Sized + AsRef<[u8]>>(input: &T) -> String {
    bs58::encode(input).into_string()
}

pub fn from_base(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    bs58::decode(s).into_vec().map_err(|err| err.into())
}

pub fn to_base64<T: ?Sized + AsRef<[u8]>>(input: &T) -> String {
    base64::encode(input)
}

pub fn from_base64(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    base64::decode(s).map_err(|err| err.into())
}

pub fn from_base_buf(s: &str, buffer: &mut Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    match bs58::decode(s).into(buffer) {
        Ok(_) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

pub fn to_str_or_base(v: &[u8]) -> String {
    match std::str::from_utf8(v) {
        Ok(res) => res.to_string(),
        _ => to_base(v),
    }
}

pub trait BaseEncode {
    fn to_base(&self) -> String;
}

impl<T> BaseEncode for T
where
    for<'a> &'a T: Into<Vec<u8>>,
{
    fn to_base(&self) -> String {
        to_base(&self.into())
    }
}

pub trait BaseDecode: for<'a> TryFrom<&'a [u8], Error = Box<dyn std::error::Error>> {
    fn from_base(s: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let bytes = from_base(s)?;
        Self::try_from(&bytes).map_err(|err| err.into())
    }
}

pub mod base_format {
    use serde::de;
    use serde::{Deserialize, Deserializer, Serializer};

    use super::{BaseDecode, BaseEncode};

    pub fn serialize<T, S>(data: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: BaseEncode,
        S: Serializer,
    {
        serializer.serialize_str(&data.to_base())
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: BaseDecode + std::fmt::Debug,
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        T::from_base(&s).map_err(|err| de::Error::custom(err.to_string()))
    }
}

pub mod vec_base_format {
    use std::fmt;

    use serde::de;
    use serde::de::{SeqAccess, Visitor};
    use serde::export::PhantomData;
    use serde::{Deserializer, Serializer};

    use crate::serde::ser::SerializeSeq;

    use super::{BaseDecode, BaseEncode};

    pub fn serialize<T, S>(data: &Vec<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: BaseEncode,
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(data.len()))?;
        for element in data {
            seq.serialize_element(&element.to_base())?;
        }
        seq.end()
    }

    struct VecBaseVisitor<T>(PhantomData<T>);

    impl<'de, T> Visitor<'de> for VecBaseVisitor<T>
    where
        T: BaseDecode,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an array with base58 in the first element")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<T>, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(elem) = seq.next_element::<String>()? {
                vec.push(T::from_base(&elem).map_err(|err| de::Error::custom(err.to_string()))?);
            }
            Ok(vec)
        }
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        T: BaseDecode + std::fmt::Debug,
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(VecBaseVisitor(PhantomData))
    }
}

pub mod base_bytes_format {
    use serde::de;
    use serde::{Deserialize, Deserializer, Serializer};

    use super::{from_base, to_base};

    pub fn serialize<S>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&to_base(data))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        from_base(&s).map_err(|err| de::Error::custom(err.to_string()))
    }
}

pub mod u128_dec_format {
    use serde::de;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(num: &u128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}", num))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u128, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        u128::from_str_radix(&s, 10).map_err(de::Error::custom)
    }
}
