use crate::types::{AccountId, Balance, BlockIndex, Gas, PublicKey};

/// Context for the contract execution.
pub struct RuntimeContext<'a> {
    /// The account id of the current contract that we are executing.
    pub current_account_id: &'a AccountId,
    /// The account id of that signed the original transaction that led to this
    /// execution.
    pub signer_account_id: &'a AccountId,
    /// The public key that was used to sign the original transaction that led to
    /// this execution.
    pub signer_account_pk: &'a PublicKey,
    /// If this execution is the result of cross-contract call or a callback then
    /// predecessor is the account that called it.
    /// If this execution is the result of direct execution of transaction then it
    /// is equal to `signer_account_id`.
    pub predecessor_account_id: &'a AccountId,
    /// The input to the contract call.
    pub input: &'a [u8],
    /// The current block index.
    pub block_index: BlockIndex,

    /// The balance attached to the given account. This includes the `attached_deposit` that was
    /// attached to the transaction.
    pub account_balance: Balance,
    /// The balance that was attached to the call that will be immediately deposited before the
    /// contract execution starts.
    pub attached_deposit: Balance,
    /// The gas attached to the call that can be used to pay for the gas fees.
    pub prepaid_gas: Gas,
    /// Initial seed for randomness
    pub random_seed: Vec<u8>,
    /// Whether the execution should not charge any costs.
    pub free_of_charge: bool,
}
