use std::collections::HashMap;

use kvdb::DBValue;

use primitives::types::{
    AccountId, AccountingInfo, AsyncCall, Balance, Callback, CallbackId,
    CallbackInfo, Mana, PromiseId, ReceiptBody, ReceiptId, ReceiptTransaction,
    Transaction,
};
use storage::{StateDbUpdate, StateDbUpdateIterator};
use wasm::ext::{Error as ExtError, External, Result as ExtResult};

use super::{account_id_to_bytes, COL_ACCOUNT, create_nonce_with_nonce};

pub struct RuntimeExt<'a> {
    state_db_update: &'a mut StateDbUpdate,
    storage_prefix: Vec<u8>,
    pub receipts: HashMap<ReceiptId, ReceiptTransaction>,
    pub callbacks: HashMap<CallbackId, Callback>,
    account_id: AccountId,
    accounting_info: AccountingInfo,
    nonce: u64,
    transaction_hash: &'a [u8],
    iters: HashMap<u32, Box<StateDbUpdateIterator<'a>>>,
    last_iter_id: u32,
}

impl<'a> RuntimeExt<'a> {
    pub fn new(
        state_db_update: &'a mut StateDbUpdate,
        account_id: &AccountId,
        accounting_info: &AccountingInfo,
        transaction_hash: &'a [u8]
    ) -> Self {
        let mut prefix = account_id_to_bytes(COL_ACCOUNT, account_id);
        prefix.append(&mut b",".to_vec());
        RuntimeExt { 
            state_db_update,
            storage_prefix: prefix,
            receipts: HashMap::new(),
            callbacks: HashMap::new(),
            account_id: account_id.clone(),
            accounting_info: accounting_info.clone(),
            nonce: 0,
            transaction_hash,
            iters: HashMap::new(),
            last_iter_id: 0,
        }
    }

    pub fn create_storage_key(&self, key: &[u8]) -> Vec<u8> {
        let mut storage_key = self.storage_prefix.clone();
        storage_key.extend_from_slice(key);
        storage_key
    }

    pub fn create_nonce(&mut self) -> Vec<u8> {
        let nonce = create_nonce_with_nonce(self.transaction_hash, self.nonce);
        self.nonce += 1;
        nonce
    }

    pub fn get_receipts(&mut self) -> Vec<Transaction> {
        self.receipts.drain().map(|(_, v)| Transaction::Receipt(v)).collect()
    }
}

impl<'a> External<'a> for RuntimeExt<'a> {
    fn storage_set(&mut self, key: &[u8], value: &[u8]) -> ExtResult<()> {
        let storage_key = self.create_storage_key(key);
        self.state_db_update.set(&storage_key, &DBValue::from_slice(value));
        Ok(())
    }

    fn storage_get(&self, key: &[u8]) -> ExtResult<Option<Vec<u8>>> {
        let storage_key = self.create_storage_key(key);
        let value = self.state_db_update.get(&storage_key);
        Ok(value.map(|buf| buf.to_vec()))
    }

    fn storage_iter(&'a mut self, prefix: &[u8]) -> ExtResult<u32> {
        let iter = self.state_db_update.iter(prefix).map_err(|_| ExtError::TrieIteratorError)?;
        self.iters.insert(self.last_iter_id, iter);
        self.last_iter_id += 1;
        Ok(self.last_iter_id - 1)
    }

    fn storage_iter_next(&mut self, id: u32) -> ExtResult<Option<Vec<u8>>> {
        let result = match self.iters.get_mut(&id) {
            Some(iter) => iter.next(),
            None => return Err(ExtError::TrieIteratorMissing),
        };
        if result.is_none() {
            self.iters.remove(&id);
        }
        Ok(result)
    }

    fn promise_create(
        &mut self,
        account_id: AccountId,
        method_name: Vec<u8>,
        arguments: Vec<u8>,
        mana: Mana,
        amount: Balance,
    ) -> ExtResult<PromiseId> {
        let nonce = self.create_nonce();
        let receipt = ReceiptTransaction::new(
            self.account_id.clone(),
            account_id,
            nonce.clone(),
            ReceiptBody::NewCall(AsyncCall::new(
                method_name,
                arguments,
                amount,
                mana,
                self.accounting_info.clone(),
            )),
        );
        let promise_id = PromiseId::Receipt(nonce.clone());
        self.receipts.insert(nonce, receipt);
        Ok(promise_id)
    }

    fn promise_then(
        &mut self,
        promise_id: PromiseId,
        method_name: Vec<u8>,
        arguments: Vec<u8>,
        mana: Mana,
    ) -> ExtResult<PromiseId> {
        let callback_id = self.create_nonce();
        let receipt_ids = match promise_id {
            PromiseId::Receipt(r) => vec![r],
            PromiseId::Joiner(rs) => rs,
            PromiseId::Callback(_) => return Err(ExtError::WrongPromise)
        };
        let mut callback = Callback::new(
            method_name,
            arguments,
            mana,
            self.accounting_info.clone(),
        );
        callback.results.resize(receipt_ids.len(), None);
        for (index, receipt_id) in receipt_ids.iter().enumerate() {
            let receipt = match self.receipts.get_mut(receipt_id) {
                Some(r) => r,
                _ => return Err(ExtError::PromiseIdNotFound)
            };
            match receipt.body {
                ReceiptBody::NewCall(ref mut async_call) => {
                    let callback_info = CallbackInfo::new(callback_id.clone(), index, self.account_id.clone());
                    match async_call.callback {
                        Some(_) => return Err(ExtError::PromiseAlreadyHasCallback),
                        None => {
                            async_call.callback = Some(callback_info);
                        }
                    }
                }
                _ => {
                    return Err(ExtError::WrongPromise);
                }
            }
        }
        self.callbacks.insert(callback_id.clone(), callback);
        Ok(PromiseId::Callback(callback_id))
    }
}