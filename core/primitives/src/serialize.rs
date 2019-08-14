use std::convert::TryFrom;
use std::io;

use byteorder::{LittleEndian, ReadBytesExt};
use serde::{de::DeserializeOwned, Serialize};
use std::io::Cursor;

pub type EncodeResult = Result<Vec<u8>, io::Error>;
pub type DecodeResult<T> = Result<T, io::Error>;

pub type WritableResult = Result<(), std::io::Error>;
pub type ReadableResult<T> = Result<T, std::io::Error>;

pub trait Writable {
    fn write(&self) -> Result<Vec<u8>, std::io::Error> {
        let mut out = vec![];
        self.write_into(&mut out)?;
        Ok(out)
    }
    fn write_into(&self, out: &mut Vec<u8>) -> WritableResult;
}

macro_rules! impl_writable_for_uint {
    ($type: ident) => {
        impl Writable for $type {
            fn write_into(&self, out: &mut Vec<u8>) -> WritableResult {
                out.extend_from_slice(&self.to_le_bytes());
                Ok(())
            }
        }
    };
}

impl_writable_for_uint!(u8);
impl_writable_for_uint!(u16);
impl_writable_for_uint!(u32);
impl_writable_for_uint!(u64);
impl_writable_for_uint!(u128);

impl<T> Writable for Vec<T>
where
    T: Writable,
{
    fn write_into(&self, out: &mut Vec<u8>) -> WritableResult {
        (self.len() as u32).write_into(out)?;
        for item in self.iter() {
            item.write_into(out)?;
        }
        Ok(())
    }
}

pub trait Readable: Sized {
    fn read(bytes: &[u8]) -> ReadableResult<Self> {
        let mut cursor = Cursor::new(bytes);
        Self::read_from_cursor(&mut cursor)
    }

    fn read_from_cursor(cursor: &mut Cursor<&[u8]>) -> ReadableResult<Self>;
}

macro_rules! impl_readable_for_primitive {
    ($type: ident, $func: ident) => {
        impl Readable for $type {
            fn read_from_cursor(cursor: &mut Cursor<&[u8]>) -> ReadableResult<Self> {
                cursor.$func()
            }
        }
    };
    ($type: ident, $func: ident, $qual: ident) => {
        impl Readable for $type {
            fn read_from_cursor(cursor: &mut Cursor<&[u8]>) -> ReadableResult<Self> {
                cursor.$func::<$qual>()
            }
        }
    };
}

impl_readable_for_primitive!(u8, read_u8);
impl_readable_for_primitive!(u16, read_u16, LittleEndian);
impl_readable_for_primitive!(u32, read_u32, LittleEndian);
impl_readable_for_primitive!(u64, read_u64, LittleEndian);
impl_readable_for_primitive!(u128, read_u128, LittleEndian);

impl<T> Readable for Vec<T>
where
    T: Readable,
{
    fn read_from_cursor(mut cursor: &mut Cursor<&[u8]>) -> ReadableResult<Self> {
        let len = u32::read_from_cursor(&mut cursor)?;
        let mut result = vec![];
        for _ in 0..len {
            result.push(T::read_from_cursor(&mut cursor)?);
        }
        Ok(result)
    }
}

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

pub mod option_bytes_format {
    use serde::de;
    use serde::{Deserialize, Deserializer, Serializer};

    use super::{from_base, to_base};

    pub fn serialize<S>(data: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(ref bytes) = data {
            serializer.serialize_str(&to_base(bytes))
        } else {
            serializer.serialize_none()
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: Option<String> = Option::deserialize(deserializer)?;
        if let Some(s) = s {
            Ok(Some(from_base(&s).map_err(|err| de::Error::custom(err.to_string()))?))
        } else {
            Ok(None)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    struct SomeStruct {
        a: u8,
        b: u32,
        c: u128,
        x: Vec<u32>,
    }

    #[derive(Deserialize, Serialize)]
    struct OptionBytesStruct {
        #[serde(with = "option_bytes_format")]
        data: Option<Vec<u8>>,
    }

    impl Writable for SomeStruct {
        fn write_into(&self, out: &mut Vec<u8>) -> WritableResult {
            self.a.write_into(out)?;
            self.b.write_into(out)?;
            self.c.write_into(out)?;
            self.x.write_into(out)
        }
    }

    impl Readable for SomeStruct {
        fn read_from_cursor(mut cursor: &mut Cursor<&[u8]>) -> ReadableResult<Self> {
            let a = u8::read_from_cursor(&mut cursor)?;
            let b = u32::read_from_cursor(&mut cursor)?;
            let c = u128::read_from_cursor(&mut cursor)?;
            let x = <Vec<u32>>::read_from_cursor(&mut cursor)?;
            Ok(Self { a, b, c, x })
        }
    }

    #[test]
    fn test_serialize_struct() {
        let s = SomeStruct { a: 1, b: 2, c: 3, x: vec![1, 2, 3] };
        let bytes = s.write().unwrap();
        let ns = SomeStruct::read(&bytes).unwrap();
        assert_eq!(s, ns);
    }

    #[test]
    fn test_serialize_some() {
        let s = OptionBytesStruct { data: Some(vec![10, 20, 30]) };
        let encoded = serde_json::to_string(&s).unwrap();
        assert_eq!(encoded, "{\"data\":\"4PM7\"}");
    }

    #[test]
    fn test_deserialize_some() {
        let encoded = "{\"data\":\"4PM7\"}";
        let decoded: OptionBytesStruct = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.data, Some(vec![10, 20, 30]));
    }

    #[test]
    fn test_serialize_none() {
        let s = OptionBytesStruct { data: None };
        let encoded = serde_json::to_string(&s).unwrap();
        assert_eq!(encoded, "{\"data\":null}");
    }

    #[test]
    fn test_deserialize_none() {
        let encoded = "{\"data\":null}";
        let decoded: OptionBytesStruct = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.data, None);
    }
}
