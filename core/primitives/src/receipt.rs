use std::borrow::Borrow;
use std::fmt;

use crate::crypto::signature::PublicKey;
use crate::hash::CryptoHash;
use crate::logging;
use crate::serialize::{option_bytes_format, u128_dec_format};
use crate::transaction::{Action, TransactionResult, TransferAction};
use crate::types::{AccountId, Balance};
use crate::utils::system_account;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReceiptInfo {
    pub receipt: Receipt,
    pub block_index: u64,
    pub result: TransactionResult,
}

#[derive(Hash, Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Receipt {
    pub predecessor_id: AccountId,
    pub receiver_id: AccountId,
    pub receipt_id: CryptoHash,

    pub receipt: ReceiptEnum,
}

impl Borrow<CryptoHash> for Receipt {
    fn borrow(&self) -> &CryptoHash {
        &self.receipt_id
    }
}

impl Receipt {
    /// It's not a content hash, but receipt_id is unique.
    pub fn get_hash(&self) -> CryptoHash {
        self.receipt_id
    }

    /// Generates a receipt with a transfer from system for a given balance without a receipt_id
    pub fn new_refund(receiver_id: &AccountId, refund: Balance) -> Self {
        Receipt {
            predecessor_id: system_account(),
            receiver_id: receiver_id.clone(),
            receipt_id: CryptoHash::default(),

            receipt: ReceiptEnum::Action(ActionReceipt {
                signer_id: system_account(),
                signer_public_key: PublicKey::empty(),
                gas_price: 0,
                output_data_receivers: vec![],
                input_data_ids: vec![],
                actions: vec![Action::Transfer(TransferAction { deposit: refund })],
            }),
        }
    }
}

#[derive(Hash, Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum ReceiptEnum {
    Action(ActionReceipt),
    Data(DataReceipt),
}

#[derive(Hash, Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ActionReceipt {
    pub signer_id: AccountId,
    pub signer_public_key: PublicKey,

    #[serde(with = "u128_dec_format")]
    pub gas_price: Balance,

    /// If present, where to route the output data
    pub output_data_receivers: Vec<DataReceiver>,

    pub input_data_ids: Vec<CryptoHash>,

    pub actions: Vec<Action>,
}

#[derive(Hash, Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct DataReceiver {
    pub data_id: CryptoHash,
    pub receiver_id: AccountId,
}

#[derive(Hash, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct DataReceipt {
    pub data_id: CryptoHash,
    #[serde(with = "option_bytes_format")]
    pub data: Option<Vec<u8>>,
}

impl fmt::Debug for DataReceipt {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("DataReceipt")
            .field("data_id", &self.data_id)
            .field("data", &format_args!("{}", logging::pretty_result(&self.data)))
            .finish()
    }
}
