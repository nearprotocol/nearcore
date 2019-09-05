//! External dependencies of the near-vm-logic.

use crate::types::{AccountId, Balance, Gas, IteratorIndex, ReceiptIndex};

/// An abstraction over the memory of the smart contract.
pub trait MemoryLike {
    /// Returns whether the memory interval is completely inside the smart contract memory.
    fn fits_memory(&self, offset: u64, len: u64) -> bool;

    /// Reads the content of the given memory interval.
    ///
    /// # Panics
    ///
    /// If memory interval is outside the smart contract memory.
    fn read_memory(&self, offset: u64, buffer: &mut [u8]);

    /// Reads a single byte from the memory.
    ///
    /// # Panics
    ///
    /// If pointer is outside the smart contract memory.
    fn read_memory_u8(&self, offset: u64) -> u8;

    /// Writes the buffer into the smart contract memory.
    ///
    /// # Panics
    ///
    /// If `offset + buffer.len()` is outside the smart contract memory.
    fn write_memory(&mut self, offset: u64, buffer: &[u8]);
}

#[derive(Debug, PartialEq, Clone)]
pub enum ExternalError {
    InvalidReceiptIndex,
    InvalidIteratorIndex,
    InvalidAccountId,
    InvalidMethodName,
    InvalidPublicKey,
}

pub type Result<T> = ::std::result::Result<T, ExternalError>;

pub trait External {
    fn storage_set(&mut self, key: &[u8], value: &[u8]) -> Result<Option<Vec<u8>>>;

    fn storage_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    fn storage_remove(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    fn storage_has_key(&mut self, key: &[u8]) -> Result<bool>;

    fn storage_iter(&mut self, prefix: &[u8]) -> Result<IteratorIndex>;

    fn storage_iter_range(&mut self, start: &[u8], end: &[u8]) -> Result<IteratorIndex>;

    fn storage_iter_next(
        &mut self,
        iterator_idx: IteratorIndex,
    ) -> Result<Option<(Vec<u8>, Vec<u8>)>>;

    fn storage_iter_drop(&mut self, iterator_idx: IteratorIndex) -> Result<()>;

    fn create_receipt(
        &mut self,
        receipt_indices: Vec<ReceiptIndex>,
        receiver_id: AccountId,
    ) -> Result<ReceiptIndex>;

    fn append_action_create_account(&mut self, receipt_index: ReceiptIndex) -> Result<()>;

    fn append_action_deploy_contract(
        &mut self,
        receipt_index: ReceiptIndex,
        code: Vec<u8>,
    ) -> Result<()>;

    fn append_action_function_call(
        &mut self,
        receipt_index: ReceiptIndex,
        method_name: Vec<u8>,
        arguments: Vec<u8>,
        attached_deposit: Balance,
        prepaid_gas: Gas,
    ) -> Result<()>;

    fn append_action_transfer(
        &mut self,
        receipt_index: ReceiptIndex,
        amount: Balance,
    ) -> Result<()>;

    fn append_action_stake(
        &mut self,
        receipt_index: ReceiptIndex,
        stake: Balance,
        public_key: Vec<u8>,
    ) -> Result<()>;

    fn append_action_add_key_with_full_access(
        &mut self,
        receipt_index: ReceiptIndex,
        public_key: Vec<u8>,
        nonce: u64,
    ) -> Result<()>;

    fn append_action_add_key_with_function_call(
        &mut self,
        receipt_index: ReceiptIndex,
        public_key: Vec<u8>,
        nonce: u64,
        allowance: Option<Balance>,
        receiver_id: AccountId,
        method_names: Vec<Vec<u8>>,
    ) -> Result<()>;

    fn append_action_delete_key(
        &mut self,
        receipt_index: ReceiptIndex,
        public_key: Vec<u8>,
    ) -> Result<()>;

    fn append_action_delete_account(
        &mut self,
        receipt_index: ReceiptIndex,
        beneficiary_id: AccountId,
    ) -> Result<()>;

    fn sha256(&self, data: &[u8]) -> Result<Vec<u8>>;
}

impl std::fmt::Display for ExternalError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        use ExternalError::*;
        match &self {
            InvalidReceiptIndex => write!(f, "VM Logic returned an invalid receipt index"),
            InvalidIteratorIndex => write!(f, "VM Logic returned an invalid iterator index"),
            InvalidAccountId => write!(f, "VM Logic returned an invalid account id"),
            InvalidMethodName => write!(f, "VM Logic returned an invalid method name"),
            InvalidPublicKey => write!(f, "VM Logic provided an invalid public key"),
        }
    }
}
