use nbor::nbor;

use crate::hash::CryptoHash;
use crate::types::{AccountId, Balance, BlockIndex, Nonce, StorageUsage};

/// Per account information stored in the state.
#[derive(nbor, PartialEq, Eq, Debug, Clone)]
pub struct Account {
    /// The sum of `amount` and `staked` is the total value of the account.
    pub amount: Balance,
    /// The amount staked by given account.
    pub staked: Balance,
    /// Hash of the code stored in the storage for this account.
    pub code_hash: CryptoHash,
    /// Storage used by the given account.
    pub storage_usage: StorageUsage,
    /// Last block index at which the storage was paid for.
    pub storage_paid_at: BlockIndex,
}

impl Account {
    pub fn new(amount: Balance, code_hash: CryptoHash, storage_paid_at: BlockIndex) -> Self {
        Account { amount, staked: 0, code_hash, storage_usage: 0, storage_paid_at }
    }

    /// Try debiting the balance by the given amount.
    pub fn checked_sub(&mut self, amount: Balance) -> Result<(), String> {
        self.amount = self.amount.checked_sub(amount).ok_or_else(|| {
            format!(
                "Sender does not have enough balance {} for operation costing {}",
                self.amount, amount
            )
        })?;
        Ok(())
    }
}

/// Access key provides limited access to an account. Each access key belongs to some account and
/// is identified by a unique (within the account) public key. One account may have large number of
/// access keys. Access keys allow to act on behalf of the account by restricting transactions
/// that can be issued.
/// `account_id,public_key` is a key in the state
#[derive(nbor, PartialEq, Eq, Hash, Clone, Debug)]
pub struct AccessKey {
    /// The nonce for this access key.
    /// NOTE: In some cases the access key needs to be recreated. If the new access key reuses the
    /// same public key, the nonce of the new access key should be equal to the nonce of the old
    /// access key. It's required to avoid replaying old transactions again.
    pub nonce: Nonce,

    /// Defines permissions for this access key.
    pub permission: AccessKeyPermission,
}

impl AccessKey {
    pub fn full_access() -> Self {
        Self { nonce: 0, permission: AccessKeyPermission::FullAccess }
    }
}

/// Defines permissions for AccessKey
#[derive(nbor, PartialEq, Eq, Hash, Clone, Debug)]
pub enum AccessKeyPermission {
    FunctionCall(FunctionCallPermission),

    /// Grants full access to the account.
    /// NOTE: It's used to replace account-level public keys.
    FullAccess,
}

/// Grants limited permission to issue transactions with a single function call action.
/// Those function calls can't have attached balance.
/// The permission can limit the allowed balance to be spent on the prepaid gas.
/// It also restrict the account ID of the receiver for this function call.
/// It also can restrict the method name for the allowed function calls.
#[derive(nbor, PartialEq, Eq, Hash, Clone, Debug)]
pub struct FunctionCallPermission {
    /// Allowance is a balance limit to use by this access key to pay for function call gas and
    /// transaction fees. When this access key is used, both account balance and the allowance is
    /// decreased by the same value.
    /// `None` means unlimited allowance.
    /// NOTE: To change or increase the allowance, the old access key needs to be deleted and a new
    /// access key should be created.
    pub allowance: Option<Balance>,

    /// The access key only allows transactions with the given receiver's account id.
    pub receiver_id: AccountId,

    /// A list of method names that can be used. The access key only allows transactions with the
    /// function call of one of the given method names.
    /// Empty list means any method name can be used.
    pub method_names: Vec<String>,
}
