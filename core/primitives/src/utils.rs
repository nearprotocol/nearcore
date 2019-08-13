use std::convert::{AsRef, TryFrom, TryInto};
use std::fmt;

use byteorder::{LittleEndian, WriteBytesExt};
use protobuf::{well_known_types::StringValue, RepeatedField, SingularPtrField};
use regex::Regex;

use lazy_static::lazy_static;

use crate::crypto::signature::PublicKey;
use crate::hash::{hash, CryptoHash};
use crate::types::{AccountId, ShardId};

pub const ACCOUNT_DATA_SEPARATOR: &[u8; 1] = b",";

pub mod col {
    pub const ACCOUNT: &[u8] = &[0];
    pub const CALLBACK: &[u8] = &[1];
    pub const CODE: &[u8] = &[2];
    pub const ACCESS_KEY: &[u8] = &[3];
    pub const RECEIVED_DATA: &[u8] = &[4];
    pub const POSTPONED_RECEIPT_ID: &[u8] = &[5];
    pub const PENDING_DATA_COUNT: &[u8] = &[6];
    pub const POSTPONED_RECEIPT: &[u8] = &[7];
}

fn key_for_column_account_id(column: &[u8], account_key: &AccountId) -> Vec<u8> {
    let mut key = column.to_vec();
    key.append(&mut account_key.clone().into_bytes());
    key
}

pub fn key_for_account(account_key: &AccountId) -> Vec<u8> {
    key_for_column_account_id(col::ACCOUNT, account_key)
}

pub fn key_for_data(account_id: &AccountId, data: &[u8]) -> Vec<u8> {
    let mut bytes = key_for_account(account_id);
    bytes.extend(ACCOUNT_DATA_SEPARATOR);
    bytes.extend(data);
    bytes
}

pub fn prefix_for_access_key(account_id: &AccountId) -> Vec<u8> {
    let mut key = key_for_column_account_id(col::ACCESS_KEY, account_id);
    key.extend_from_slice(col::ACCESS_KEY);
    key
}

pub fn prefix_for_data(account_id: &AccountId) -> Vec<u8> {
    let mut prefix = key_for_account(account_id);
    prefix.append(&mut ACCOUNT_DATA_SEPARATOR.to_vec());
    prefix
}

pub fn key_for_access_key(account_id: &AccountId, public_key: &PublicKey) -> Vec<u8> {
    let mut key = key_for_column_account_id(col::ACCESS_KEY, account_id);
    key.extend_from_slice(col::ACCESS_KEY);
    key.extend_from_slice(public_key.as_ref());
    key
}

pub fn key_for_code(account_key: &AccountId) -> Vec<u8> {
    key_for_column_account_id(col::CODE, account_key)
}

pub fn key_for_received_data(account_id: &AccountId, data_id: &CryptoHash) -> Vec<u8> {
    let mut key = key_for_column_account_id(col::RECEIVED_DATA, account_id);
    key.append(&mut ACCOUNT_DATA_SEPARATOR.to_vec());
    key.extend_from_slice(data_id.as_ref());
    key
}

pub fn key_for_postponed_receipt_id(account_id: &AccountId, data_id: &CryptoHash) -> Vec<u8> {
    let mut key = key_for_column_account_id(col::POSTPONED_RECEIPT_ID, account_id);
    key.append(&mut ACCOUNT_DATA_SEPARATOR.to_vec());
    key.extend_from_slice(data_id.as_ref());
    key
}

pub fn key_for_pending_data_count(account_id: &AccountId, receipt_id: &CryptoHash) -> Vec<u8> {
    let mut key = key_for_column_account_id(col::PENDING_DATA_COUNT, account_id);
    key.append(&mut ACCOUNT_DATA_SEPARATOR.to_vec());
    key.extend_from_slice(receipt_id.as_ref());
    key
}

pub fn key_for_postponed_receipt(account_id: &AccountId, receipt_id: &CryptoHash) -> Vec<u8> {
    let mut key = key_for_column_account_id(col::POSTPONED_RECEIPT, account_id);
    key.append(&mut ACCOUNT_DATA_SEPARATOR.to_vec());
    key.extend_from_slice(receipt_id.as_ref());
    key
}

pub fn create_nonce_with_nonce(base: &CryptoHash, salt: u64) -> CryptoHash {
    let mut nonce: Vec<u8> = base.as_ref().to_owned();
    nonce.append(&mut index_to_bytes(salt));
    hash(&nonce)
}

pub fn index_to_bytes(index: u64) -> Vec<u8> {
    let mut bytes = vec![];
    bytes.write_u64::<LittleEndian>(index).expect("writing to bytes failed");
    bytes
}

#[allow(unused)]
pub fn account_to_shard_id(account_id: &AccountId) -> ShardId {
    // TODO: change to real sharding
    0
}

lazy_static! {
    static ref VALID_ACCOUNT_ID: Regex = Regex::new(r"^[a-z0-9@._\-]{5,32}$").unwrap();
}

/// const does not allow function call, so have to resort to this
pub fn system_account() -> AccountId {
    "system".to_string()
}

pub fn is_valid_account_id(account_id: &AccountId) -> bool {
    if *account_id == system_account() {
        return false;
    }
    VALID_ACCOUNT_ID.is_match(account_id)
}

pub fn to_string_value(s: String) -> StringValue {
    let mut res = StringValue::new();
    res.set_value(s);
    res
}

pub fn proto_to_result<T>(proto: SingularPtrField<T>) -> Result<T, Box<dyn std::error::Error>> {
    proto.into_option().ok_or_else(|| "Bad Proto".into())
}

pub fn proto_to_type<T, U>(proto: SingularPtrField<T>) -> Result<U, Box<dyn std::error::Error>>
where
    U: TryFrom<T, Error = Box<dyn std::error::Error>>,
{
    proto_to_result(proto).and_then(TryInto::try_into)
}

pub fn proto_to_vec<T, U>(proto: RepeatedField<T>) -> Result<Vec<U>, Box<dyn std::error::Error>>
where
    U: TryFrom<T, Error = Box<dyn std::error::Error>>,
{
    proto.into_iter().map(|v| v.try_into()).collect::<Result<Vec<_>, _>>()
}

/// A wrapper around Option<T> that provides native Display trait.
/// Simplifies propagating automatic Display trait on parent structs.
pub struct DisplayOption<T>(pub Option<T>);

impl<T: fmt::Display> fmt::Display for DisplayOption<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(ref v) => write!(f, "Some({})", v),
            None => write!(f, "None"),
        }
    }
}

impl<T> DisplayOption<T> {
    pub fn into(self) -> Option<T> {
        self.0
    }
}

impl<T> AsRef<Option<T>> for DisplayOption<T> {
    fn as_ref(&self) -> &Option<T> {
        &self.0
    }
}

impl<T: fmt::Display> From<Option<T>> for DisplayOption<T> {
    fn from(o: Option<T>) -> Self {
        DisplayOption(o)
    }
}

/// Macro to either return value if the result is Ok, or exit function logging error.
#[macro_export]
macro_rules! unwrap_or_return(($obj: expr, $ret: expr) => (match $obj {
    Ok(value) => value,
    Err(err) => {
        error!(target: "client", "Unwrap error: {}", err);
        return $ret;
    }
}));
