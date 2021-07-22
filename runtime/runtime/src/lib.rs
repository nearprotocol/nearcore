use std::cmp::max;
use std::collections::{HashMap, HashSet};

use log::debug;

use near_chain_configs::Genesis;
pub use near_crypto;
use near_crypto::PublicKey;
pub use near_primitives;
use near_primitives::runtime::get_insufficient_storage_stake;
use near_primitives::{
    account::Account,
    checked_feature,
    errors::{ActionError, ActionErrorKind, RuntimeError, TxExecutionError},
    hash::CryptoHash,
    receipt::{
        ActionReceipt, DataReceipt, DelayedReceiptIndices, Receipt, ReceiptEnum, ReceivedData,
    },
    state_record::StateRecord,
    transaction::{
        Action, ExecutionOutcome, ExecutionOutcomeWithId, ExecutionStatus, LogEntry,
        SignedTransaction,
    },
    trie_key::TrieKey,
    types::{
        validator_stake::ValidatorStake, AccountId, Balance, EpochInfoProvider, Gas,
        RawStateChangesWithTrieKey, ShardId, StateChangeCause, StateRoot,
    },
    utils::{
        create_action_hash, create_receipt_id_from_receipt, create_receipt_id_from_transaction,
        system_account,
    },
};
use near_store::{
    get, get_account, get_postponed_receipt, get_received_data, remove_postponed_receipt, set,
    set_account, set_postponed_receipt, set_received_data, PartialStorage, ShardTries,
    StorageError, Trie, TrieChanges, TrieUpdate,
};
#[cfg(feature = "sandbox")]
use near_store::{set_access_key, set_code};
use near_vm_logic::types::PromiseResult;
use near_vm_logic::ReturnData;
pub use near_vm_runner::with_ext_cost_counter;

use crate::actions::*;
use crate::balance_checker::check_balance;
use crate::config::{
    exec_fee, safe_add_balance, safe_add_gas, safe_gas_to_balance, total_deposit,
    total_prepaid_exec_fees, total_prepaid_gas, RuntimeConfig,
};
use crate::genesis::{GenesisStateApplier, StorageComputer};
use crate::verifier::validate_receipt;
pub use crate::verifier::{validate_transaction, verify_and_charge_transaction};
#[cfg(feature = "sandbox")]
use near_primitives::contract::ContractCode;
pub use near_primitives::runtime::apply_state::ApplyState;
use near_primitives::runtime::fees::RuntimeFeesConfig;
use near_primitives::runtime::migration_data::{MigrationData, MigrationFlags};
use near_primitives::transaction::ExecutionMetadata;
use near_primitives::version::{
    is_implicit_account_creation_enabled, ProtocolFeature, ProtocolVersion,
};
use std::rc::Rc;
use std::sync::Arc;

mod actions;
pub mod adapter;
pub mod balance_checker;
pub mod cache;
pub mod config;
pub mod ext;
mod genesis;
mod metrics;
pub mod state_viewer;
pub mod verifier;

const EXPECT_ACCOUNT_EXISTS: &str = "account exists, checked above";

/// Contains information to update validators accounts at the first block of a new epoch.
#[derive(Debug)]
pub struct ValidatorAccountsUpdate {
    /// Maximum stake across last 3 epochs.
    pub stake_info: HashMap<AccountId, Balance>,
    /// Rewards to distribute to validators.
    pub validator_rewards: HashMap<AccountId, Balance>,
    /// Stake proposals from the last chunk.
    pub last_proposals: HashMap<AccountId, Balance>,
    /// The ID of the protocol treasure account if it belongs to the current shard.
    pub protocol_treasury_account_id: Option<AccountId>,
    /// Accounts to slash and the slashed amount (None means everything)
    pub slashing_info: HashMap<AccountId, Option<Balance>>,
}

#[derive(Debug)]
pub struct VerificationResult {
    /// The amount gas that was burnt to convert the transaction into a receipt and send it.
    pub gas_burnt: Gas,
    /// The remaining amount of gas in the receipt.
    pub gas_remaining: Gas,
    /// The gas price at which the gas was purchased in the receipt.
    pub receipt_gas_price: Balance,
    /// The balance that was burnt to convert the transaction into a receipt and send it.
    pub burnt_amount: Balance,
}

#[derive(Debug, Default)]
pub struct ApplyStats {
    pub tx_burnt_amount: Balance,
    pub slashed_burnt_amount: Balance,
    pub other_burnt_amount: Balance,
    /// This is a negative amount. This amount was not charged from the account that issued
    /// the transaction. It's likely due to the delayed queue of the receipts.
    pub gas_deficit_amount: Balance,
}

pub struct ApplyResult {
    pub state_root: StateRoot,
    pub trie_changes: TrieChanges,
    pub validator_proposals: Vec<ValidatorStake>,
    pub outgoing_receipts: Vec<Receipt>,
    pub outcomes: Vec<ExecutionOutcomeWithId>,
    pub state_changes: Vec<RawStateChangesWithTrieKey>,
    pub stats: ApplyStats,
    pub proof: Option<PartialStorage>,
}

#[derive(Debug)]
pub struct ActionResult {
    pub gas_burnt: Gas,
    pub gas_burnt_for_function_call: Gas,
    pub gas_used: Gas,
    pub result: Result<ReturnData, ActionError>,
    pub logs: Vec<LogEntry>,
    pub new_receipts: Vec<Receipt>,
    pub validator_proposals: Vec<ValidatorStake>,
}

impl ActionResult {
    pub fn merge(&mut self, mut next_result: ActionResult) -> Result<(), RuntimeError> {
        assert!(next_result.gas_burnt_for_function_call <= next_result.gas_burnt);
        assert!(
            next_result.gas_burnt <= next_result.gas_used,
            "Gas burnt {} <= Gas used {}",
            next_result.gas_burnt,
            next_result.gas_used
        );
        self.gas_burnt = safe_add_gas(self.gas_burnt, next_result.gas_burnt)?;
        self.gas_burnt_for_function_call = safe_add_gas(
            self.gas_burnt_for_function_call,
            next_result.gas_burnt_for_function_call,
        )?;
        self.gas_used = safe_add_gas(self.gas_used, next_result.gas_used)?;
        self.result = next_result.result;
        self.logs.append(&mut next_result.logs);
        if let Ok(ReturnData::ReceiptIndex(ref mut receipt_index)) = self.result {
            // Shifting local receipt index to be global receipt index.
            *receipt_index += self.new_receipts.len() as u64;
        }
        if self.result.is_ok() {
            self.new_receipts.append(&mut next_result.new_receipts);
            self.validator_proposals.append(&mut next_result.validator_proposals);
        } else {
            self.new_receipts.clear();
            self.validator_proposals.clear();
        }
        Ok(())
    }
}

impl Default for ActionResult {
    fn default() -> Self {
        Self {
            gas_burnt: 0,
            gas_burnt_for_function_call: 0,
            gas_used: 0,
            result: Ok(ReturnData::None),
            logs: vec![],
            new_receipts: vec![],
            validator_proposals: vec![],
        }
    }
}

pub struct Runtime {}

impl Runtime {
    pub fn new() -> Self {
        Self {}
    }

    fn print_log(log: &[LogEntry]) {
        if log.is_empty() {
            return;
        }
        let log_str = log.iter().fold(String::new(), |acc, s| {
            if acc.is_empty() {
                s.to_string()
            } else {
                acc + "\n" + s
            }
        });
        debug!(target: "runtime", "{}", log_str);
    }

    /// Takes one signed transaction, verifies it and converts it to a receipt. Add this receipt
    /// either to the new local receipts if the signer is the same as receiver or to the new
    /// outgoing receipts.
    /// When transaction is converted to a receipt, the account is charged for the full value of
    /// the generated receipt.
    /// In case of successful verification (expected for valid chunks), returns the receipt and
    /// `ExecutionOutcomeWithId` for the transaction.
    /// In case of an error, returns either `InvalidTxError` if the transaction verification failed
    /// or a `StorageError` wrapped into `RuntimeError`.
    fn process_transaction(
        &self,
        state_update: &mut TrieUpdate,
        apply_state: &ApplyState,
        signed_transaction: &SignedTransaction,
        stats: &mut ApplyStats,
    ) -> Result<(Receipt, ExecutionOutcomeWithId), RuntimeError> {
        let _span =
            tracing::debug_span!(target: "runtime", "Runtime::process_transaction").entered();
        near_metrics::inc_counter(&metrics::TRANSACTION_PROCESSED_TOTAL);

        match verify_and_charge_transaction(
            &apply_state.config,
            state_update,
            apply_state.gas_price,
            signed_transaction,
            true,
            Some(apply_state.block_index),
            apply_state.current_protocol_version,
        ) {
            Ok(verification_result) => {
                near_metrics::inc_counter(&metrics::TRANSACTION_PROCESSED_SUCCESSFULLY_TOTAL);
                state_update.commit(StateChangeCause::TransactionProcessing {
                    tx_hash: signed_transaction.get_hash(),
                });
                let transaction = &signed_transaction.transaction;
                let receipt_id = create_receipt_id_from_transaction(
                    apply_state.current_protocol_version,
                    &signed_transaction,
                    &apply_state.prev_block_hash,
                    &apply_state.block_hash,
                );
                let receipt = Receipt {
                    predecessor_id: transaction.signer_id.clone(),
                    receiver_id: transaction.receiver_id.clone(),
                    receipt_id,
                    receipt: ReceiptEnum::Action(ActionReceipt {
                        signer_id: transaction.signer_id.clone(),
                        signer_public_key: transaction.public_key.clone(),
                        gas_price: verification_result.receipt_gas_price,
                        output_data_receivers: vec![],
                        input_data_ids: vec![],
                        actions: transaction.actions.clone(),
                    }),
                };
                stats.tx_burnt_amount =
                    safe_add_balance(stats.tx_burnt_amount, verification_result.burnt_amount)?;
                let outcome = ExecutionOutcomeWithId {
                    id: signed_transaction.get_hash(),
                    outcome: ExecutionOutcome {
                        status: ExecutionStatus::SuccessReceiptId(receipt.receipt_id),
                        logs: vec![],
                        receipt_ids: vec![receipt.receipt_id],
                        gas_burnt: verification_result.gas_burnt,
                        tokens_burnt: verification_result.burnt_amount,
                        executor_id: transaction.signer_id.clone(),
                        metadata: ExecutionMetadata::ExecutionMetadataV1,
                    },
                };
                Ok((receipt, outcome))
            }
            Err(e) => {
                near_metrics::inc_counter(&metrics::TRANSACTION_PROCESSED_FAILED_TOTAL);
                state_update.rollback();
                return Err(e);
            }
        }
    }

    fn apply_action(
        &self,
        action: &Action,
        state_update: &mut TrieUpdate,
        apply_state: &ApplyState,
        account: &mut Option<Account>,
        actor_id: &mut AccountId,
        receipt: &Receipt,
        action_receipt: &ActionReceipt,
        promise_results: &[PromiseResult],
        action_hash: &CryptoHash,
        action_index: usize,
        actions: &[Action],
        epoch_info_provider: &dyn EpochInfoProvider,
    ) -> Result<ActionResult, RuntimeError> {
        // println!("enter apply_action");
        let mut result = ActionResult::default();
        let exec_fees = exec_fee(
            &apply_state.config.transaction_costs,
            action,
            &receipt.receiver_id,
            apply_state.current_protocol_version,
        );
        result.gas_burnt += exec_fees;
        result.gas_used += exec_fees;
        let account_id = &receipt.receiver_id;
        let is_the_only_action = actions.len() == 1;
        let is_refund = receipt.predecessor_id == system_account();
        // Account validation
        if let Err(e) = check_account_existence(
            action,
            account,
            account_id,
            apply_state.current_protocol_version,
            is_the_only_action,
            is_refund,
        ) {
            result.result = Err(e);
            return Ok(result);
        }
        // Permission validation
        if let Err(e) = check_actor_permissions(action, account, &actor_id, account_id) {
            result.result = Err(e);
            return Ok(result);
        }
        match action {
            Action::CreateAccount(_) => {
                near_metrics::inc_counter(&metrics::ACTION_CREATE_ACCOUNT_TOTAL);
                action_create_account(
                    &apply_state.config.transaction_costs,
                    &apply_state.config.account_creation_config,
                    account,
                    actor_id,
                    &receipt.receiver_id,
                    &receipt.predecessor_id,
                    &mut result,
                );
            }
            Action::DeployContract(deploy_contract) => {
                near_metrics::inc_counter(&metrics::ACTION_DEPLOY_CONTRACT_TOTAL);
                action_deploy_contract(
                    state_update,
                    account.as_mut().expect(EXPECT_ACCOUNT_EXISTS),
                    &account_id,
                    deploy_contract,
                    &apply_state,
                )?;
            }
            Action::FunctionCall(function_call) => {
                near_metrics::inc_counter(&metrics::ACTION_FUNCTION_CALL_TOTAL);
                action_function_call(
                    state_update,
                    apply_state,
                    account.as_mut().expect(EXPECT_ACCOUNT_EXISTS),
                    receipt,
                    action_receipt,
                    promise_results,
                    &mut result,
                    account_id,
                    function_call,
                    action_hash,
                    &apply_state.config,
                    action_index + 1 == actions.len(),
                    epoch_info_provider,
                )?;
            }
            Action::Transfer(transfer) => {
                near_metrics::inc_counter(&metrics::ACTION_TRANSFER_TOTAL);
                if let Some(account) = account.as_mut() {
                    action_transfer(account, transfer)?;
                    // Check if this is a gas refund, then try to refund the access key allowance.
                    if is_refund && action_receipt.signer_id == receipt.receiver_id {
                        try_refund_allowance(
                            state_update,
                            &receipt.receiver_id,
                            &action_receipt.signer_public_key,
                            transfer,
                        )?;
                    }
                } else {
                    // Implicit account creation
                    debug_assert!(is_implicit_account_creation_enabled(
                        apply_state.current_protocol_version
                    ));
                    debug_assert!(!is_refund);
                    action_implicit_account_creation_transfer(
                        state_update,
                        &apply_state.config.transaction_costs,
                        account,
                        actor_id,
                        &receipt.receiver_id,
                        transfer,
                    );
                }
            }
            Action::Stake(stake) => {
                near_metrics::inc_counter(&metrics::ACTION_STAKE_TOTAL);
                action_stake(
                    account.as_mut().expect(EXPECT_ACCOUNT_EXISTS),
                    &mut result,
                    account_id,
                    stake,
                    &apply_state.prev_block_hash,
                    epoch_info_provider,
                )?;
            }
            Action::AddKey(add_key) => {
                near_metrics::inc_counter(&metrics::ACTION_ADD_KEY_TOTAL);
                action_add_key(
                    apply_state,
                    state_update,
                    account.as_mut().expect(EXPECT_ACCOUNT_EXISTS),
                    &mut result,
                    account_id,
                    add_key,
                )?;
            }
            Action::DeleteKey(delete_key) => {
                near_metrics::inc_counter(&metrics::ACTION_DELETE_KEY_TOTAL);
                action_delete_key(
                    &apply_state.config.transaction_costs,
                    state_update,
                    account.as_mut().expect(EXPECT_ACCOUNT_EXISTS),
                    &mut result,
                    account_id,
                    delete_key,
                    apply_state.current_protocol_version,
                )?;
            }
            Action::DeleteAccount(delete_account) => {
                near_metrics::inc_counter(&metrics::ACTION_DELETE_ACCOUNT_TOTAL);
                action_delete_account(
                    state_update,
                    account,
                    actor_id,
                    receipt,
                    &mut result,
                    account_id,
                    delete_account,
                    apply_state.current_protocol_version,
                )?;
            }
        };
        Ok(result)
    }

    // Executes when all Receipt `input_data_ids` are in the state
    fn apply_action_receipt(
        &self,
        state_update: &mut TrieUpdate,
        apply_state: &ApplyState,
        receipt: &Receipt,
        outgoing_receipts: &mut Vec<Receipt>,
        validator_proposals: &mut Vec<ValidatorStake>,
        stats: &mut ApplyStats,
        epoch_info_provider: &dyn EpochInfoProvider,
    ) -> Result<ExecutionOutcomeWithId, RuntimeError> {
        let action_receipt = match &receipt.receipt {
            ReceiptEnum::Action(action_receipt) => action_receipt,
            _ => unreachable!("given receipt should be an action receipt"),
        };
        let account_id = &receipt.receiver_id;
        // Collecting input data and removing it from the state
        let promise_results = action_receipt
            .input_data_ids
            .iter()
            .map(|data_id| {
                let ReceivedData { data } = get_received_data(state_update, account_id, *data_id)?
                    .ok_or_else(|| {
                        StorageError::StorageInconsistentState(
                            "received data should be in the state".to_string(),
                        )
                    })?;
                state_update.remove(TrieKey::ReceivedData {
                    receiver_id: account_id.clone(),
                    data_id: *data_id,
                });
                match data {
                    Some(value) => Ok(PromiseResult::Successful(value)),
                    None => Ok(PromiseResult::Failed),
                }
            })
            .collect::<Result<Vec<PromiseResult>, RuntimeError>>()?;

        // state_update might already have some updates so we need to make sure we commit it before
        // executing the actual receipt
        state_update.commit(StateChangeCause::ActionReceiptProcessingStarted {
            receipt_hash: receipt.get_hash(),
        });

        let mut account = get_account(state_update, account_id)?;
        let mut actor_id = receipt.predecessor_id.clone();
        let mut result = ActionResult::default();
        let exec_fee =
            apply_state.config.transaction_costs.action_receipt_creation_config.exec_fee();
        result.gas_used = exec_fee;
        result.gas_burnt = exec_fee;
        // Executing actions one by one
        for (action_index, action) in action_receipt.actions.iter().enumerate() {
            let action_hash = create_action_hash(
                apply_state.current_protocol_version,
                &receipt,
                &apply_state.prev_block_hash,
                &apply_state.block_hash,
                action_index,
            );
            let mut new_result = self.apply_action(
                action,
                state_update,
                apply_state,
                &mut account,
                &mut actor_id,
                receipt,
                action_receipt,
                &promise_results,
                &action_hash,
                action_index,
                &action_receipt.actions,
                epoch_info_provider,
            )?;
            if new_result.result.is_ok() {
                if let Err(e) = new_result.new_receipts.iter().try_for_each(|receipt| {
                    validate_receipt(&apply_state.config.wasm_config.limit_config, receipt)
                }) {
                    new_result.result = Err(ActionErrorKind::NewReceiptValidationError(e).into());
                }
            }
            result.merge(new_result)?;
            // TODO storage error
            if let Err(ref mut res) = result.result {
                res.index = Some(action_index as u64);
                break;
            }
        }

        // Going to check balance covers account's storage.
        if result.result.is_ok() {
            if let Some(ref mut account) = account {
                if let Some(amount) = get_insufficient_storage_stake(account, &apply_state.config)
                    .map_err(|err| StorageError::StorageInconsistentState(err))?
                {
                    result.merge(ActionResult {
                        result: Err(ActionError {
                            index: None,
                            kind: ActionErrorKind::LackBalanceForState {
                                account_id: account_id.clone(),
                                amount,
                            },
                        }),
                        ..Default::default()
                    })?;
                } else {
                    set_account(state_update, account_id.clone(), account);
                }
            }
        }

        let gas_deficit_amount = if receipt.predecessor_id == system_account() {
            // We will set gas_burnt for refund receipts to be 0 when we calculate tx_burnt_amount
            // Here we don't set result.gas_burnt to be zero if CountRefundReceiptsInGasLimit is
            // enabled because we want it to be counted in gas limit calculation later
            if !checked_feature!(
                "stable",
                CountRefundReceiptsInGasLimit,
                apply_state.current_protocol_version
            ) {
                result.gas_burnt = 0;
                result.gas_used = 0;
            }
            // If the refund fails tokens are burned.
            if result.result.is_err() {
                stats.other_burnt_amount = safe_add_balance(
                    stats.other_burnt_amount,
                    total_deposit(&action_receipt.actions)?,
                )?
            }
            0
        } else {
            // Calculating and generating refunds
            self.generate_refund_receipts(
                apply_state.gas_price,
                receipt,
                action_receipt,
                &mut result,
                apply_state.current_protocol_version,
                &apply_state.config.transaction_costs,
            )?
        };
        stats.gas_deficit_amount = safe_add_balance(stats.gas_deficit_amount, gas_deficit_amount)?;

        // Moving validator proposals
        validator_proposals.append(&mut result.validator_proposals);

        // Committing or rolling back state.
        match &result.result {
            Ok(_) => {
                state_update.commit(StateChangeCause::ReceiptProcessing {
                    receipt_hash: receipt.get_hash(),
                });
            }
            Err(_) => {
                state_update.rollback();
            }
        };

        // If the receipt is a refund, then we consider it free without burnt gas.
        let gas_burnt: Gas =
            if receipt.predecessor_id == system_account() { 0 } else { result.gas_burnt };
        // `gas_deficit_amount` is strictly less than `gas_price * gas_burnt`.
        let mut tx_burnt_amount =
            safe_gas_to_balance(apply_state.gas_price, gas_burnt)? - gas_deficit_amount;
        // The amount of tokens burnt for the execution of this receipt. It's used in the execution
        // outcome.
        let tokens_burnt = tx_burnt_amount;

        // Adding burnt gas reward for function calls if the account exists.
        let receiver_gas_reward = result.gas_burnt_for_function_call
            * *apply_state.config.transaction_costs.burnt_gas_reward.numer() as u64
            / *apply_state.config.transaction_costs.burnt_gas_reward.denom() as u64;
        // The balance that the current account should receive as a reward for function call
        // execution.
        let receiver_reward = safe_gas_to_balance(apply_state.gas_price, receiver_gas_reward)?
            .saturating_sub(gas_deficit_amount);
        if receiver_reward > 0 {
            let mut account = get_account(state_update, account_id)?;
            if let Some(ref mut account) = account {
                // Validators receive the remaining execution reward that was not given to the
                // account holder. If the account doesn't exist by the end of the execution, the
                // validators receive the full reward.
                tx_burnt_amount -= receiver_reward;
                account.set_amount(safe_add_balance(account.amount(), receiver_reward)?);
                set_account(state_update, account_id.clone(), account);
                state_update.commit(StateChangeCause::ActionReceiptGasReward {
                    receipt_hash: receipt.get_hash(),
                });
            }
        }

        stats.tx_burnt_amount = safe_add_balance(stats.tx_burnt_amount, tx_burnt_amount)?;

        // Generating outgoing data
        // A {
        // B().then(C())}  B--data receipt->C

        // A {
        // B(); 42}
        if !action_receipt.output_data_receivers.is_empty() {
            if let Ok(ReturnData::ReceiptIndex(receipt_index)) = result.result {
                // Modifying a new receipt instead of sending data
                match result
                    .new_receipts
                    .get_mut(receipt_index as usize)
                    .expect("the receipt for the given receipt index should exist")
                    .receipt
                {
                    ReceiptEnum::Action(ref mut new_action_receipt) => new_action_receipt
                        .output_data_receivers
                        .extend_from_slice(&action_receipt.output_data_receivers),
                    _ => unreachable!("the receipt should be an action receipt"),
                }
            } else {
                let data = match result.result {
                    Ok(ReturnData::Value(ref data)) => Some(data.clone()),
                    Ok(_) => Some(vec![]),
                    Err(_) => None,
                };
                result.new_receipts.extend(action_receipt.output_data_receivers.iter().map(
                    |data_receiver| Receipt {
                        predecessor_id: account_id.clone(),
                        receiver_id: data_receiver.receiver_id.clone(),
                        receipt_id: CryptoHash::default(),
                        receipt: ReceiptEnum::Data(DataReceipt {
                            data_id: data_receiver.data_id,
                            data: data.clone(),
                        }),
                    },
                ));
            };
        }

        // Generating receipt IDs
        let receipt_ids = result
            .new_receipts
            .into_iter()
            .enumerate()
            .filter_map(|(receipt_index, mut new_receipt)| {
                let receipt_id = create_receipt_id_from_receipt(
                    apply_state.current_protocol_version,
                    &receipt,
                    &apply_state.prev_block_hash,
                    &apply_state.block_hash,
                    receipt_index,
                );

                new_receipt.receipt_id = receipt_id;
                let is_action = match &new_receipt.receipt {
                    ReceiptEnum::Action(_) => true,
                    _ => false,
                };
                outgoing_receipts.push(new_receipt);
                if is_action {
                    Some(receipt_id)
                } else {
                    None
                }
            })
            .collect();

        let status = match result.result {
            Ok(ReturnData::ReceiptIndex(receipt_index)) => {
                ExecutionStatus::SuccessReceiptId(create_receipt_id_from_receipt(
                    apply_state.current_protocol_version,
                    &receipt,
                    &apply_state.prev_block_hash,
                    &apply_state.block_hash,
                    receipt_index as usize,
                ))
            }
            Ok(ReturnData::Value(data)) => ExecutionStatus::SuccessValue(data),
            Ok(ReturnData::None) => ExecutionStatus::SuccessValue(vec![]),
            Err(e) => ExecutionStatus::Failure(TxExecutionError::ActionError(e)),
        };

        Self::print_log(&result.logs);

        Ok(ExecutionOutcomeWithId {
            id: receipt.receipt_id,
            outcome: ExecutionOutcome {
                status,
                logs: result.logs,
                receipt_ids,
                gas_burnt: result.gas_burnt,
                tokens_burnt,
                executor_id: account_id.clone(),
                metadata: ExecutionMetadata::ExecutionMetadataV1,
            },
        })
    }

    fn generate_refund_receipts(
        &self,
        current_gas_price: Balance,
        receipt: &Receipt,
        action_receipt: &ActionReceipt,
        result: &mut ActionResult,
        current_protocol_version: ProtocolVersion,
        transaction_costs: &RuntimeFeesConfig,
    ) -> Result<Balance, RuntimeError> {
        let total_deposit = total_deposit(&action_receipt.actions)?;
        let prepaid_gas = total_prepaid_gas(&action_receipt.actions)?;
        let prepaid_exec_gas = safe_add_gas(
            total_prepaid_exec_fees(
                &transaction_costs,
                &action_receipt.actions,
                &receipt.receiver_id,
                current_protocol_version,
            )?,
            transaction_costs.action_receipt_creation_config.exec_fee(),
        )?;
        let deposit_refund = if result.result.is_err() { total_deposit } else { 0 };
        let gas_refund = if result.result.is_err() {
            safe_add_gas(prepaid_gas, prepaid_exec_gas)? - result.gas_burnt
        } else {
            safe_add_gas(prepaid_gas, prepaid_exec_gas)? - result.gas_used
        };
        // Refund for the unused portion of the gas at the price at which this gas was purchased.
        let mut gas_balance_refund = safe_gas_to_balance(action_receipt.gas_price, gas_refund)?;
        let mut gas_deficit_amount = 0;
        if current_gas_price > action_receipt.gas_price {
            // In a rare scenario, when the current gas price is higher than the purchased gas
            // price, the difference is subtracted from the refund. If the refund doesn't have
            // enough balance to cover the difference, then the remaining balance is considered
            // the deficit and it's reported in the stats for the balance checker.
            gas_deficit_amount = safe_gas_to_balance(
                current_gas_price - action_receipt.gas_price,
                result.gas_burnt,
            )?;
            if gas_balance_refund >= gas_deficit_amount {
                gas_balance_refund -= gas_deficit_amount;
                gas_deficit_amount = 0;
            } else {
                gas_deficit_amount -= gas_balance_refund;
                gas_balance_refund = 0;
            }
        } else {
            // Refund for the difference of the purchased gas price and the the current gas price.
            gas_balance_refund = safe_add_balance(
                gas_balance_refund,
                safe_gas_to_balance(
                    action_receipt.gas_price - current_gas_price,
                    result.gas_burnt,
                )?,
            )?;
        }
        if deposit_refund > 0 {
            result
                .new_receipts
                .push(Receipt::new_balance_refund(&receipt.predecessor_id, deposit_refund));
        }
        if gas_balance_refund > 0 {
            // Gas refunds refund the allowance of the access key, so if the key exists on the
            // account it will increase the allowance by the refund amount.
            result.new_receipts.push(Receipt::new_gas_refund(
                &action_receipt.signer_id,
                gas_balance_refund,
                action_receipt.signer_public_key.clone(),
            ));
        }
        Ok(gas_deficit_amount)
    }

    fn process_receipt(
        &self,
        state_update: &mut TrieUpdate,
        apply_state: &ApplyState,
        receipt: &Receipt,
        outgoing_receipts: &mut Vec<Receipt>,
        validator_proposals: &mut Vec<ValidatorStake>,
        stats: &mut ApplyStats,
        epoch_info_provider: &dyn EpochInfoProvider,
    ) -> Result<Option<ExecutionOutcomeWithId>, RuntimeError> {
        let _span = tracing::debug_span!(target: "runtime", "Runtime::process_receipt").entered();

        let account_id = &receipt.receiver_id;
        match receipt.receipt {
            ReceiptEnum::Data(ref data_receipt) => {
                // Received a new data receipt.
                // Saving the data into the state keyed by the data_id.
                set_received_data(
                    state_update,
                    account_id.clone(),
                    data_receipt.data_id,
                    &ReceivedData { data: data_receipt.data.clone() },
                );
                // Check if there is already a receipt that was postponed and was awaiting for the
                // given data_id.
                // If we don't have a postponed receipt yet, we don't need to do anything for now.
                if let Some(receipt_id) = get(
                    state_update,
                    &TrieKey::PostponedReceiptId {
                        receiver_id: account_id.clone(),
                        data_id: data_receipt.data_id,
                    },
                )? {
                    // There is already a receipt that is awaiting for the just received data.
                    // Removing this pending data_id for the receipt from the state.
                    state_update.remove(TrieKey::PostponedReceiptId {
                        receiver_id: account_id.clone(),
                        data_id: data_receipt.data_id,
                    });
                    // Checking how many input data items is pending for the receipt.
                    let pending_data_count: u32 = get(
                        state_update,
                        &TrieKey::PendingDataCount { receiver_id: account_id.clone(), receipt_id },
                    )?
                    .ok_or_else(|| {
                        StorageError::StorageInconsistentState(
                            "pending data count should be in the state".to_string(),
                        )
                    })?;
                    if pending_data_count == 1 {
                        // It was the last input data pending for this receipt. We'll cleanup
                        // some receipt related fields from the state and execute the receipt.

                        // Removing pending data count from the state.
                        state_update.remove(TrieKey::PendingDataCount {
                            receiver_id: account_id.clone(),
                            receipt_id,
                        });
                        // Fetching the receipt itself.
                        let ready_receipt =
                            get_postponed_receipt(state_update, account_id, receipt_id)?
                                .ok_or_else(|| {
                                    StorageError::StorageInconsistentState(
                                        "pending receipt should be in the state".to_string(),
                                    )
                                })?;
                        // Removing the receipt from the state.
                        remove_postponed_receipt(state_update, account_id, receipt_id);
                        // Executing the receipt. It will read all the input data and clean it up
                        // from the state.
                        return self
                            .apply_action_receipt(
                                state_update,
                                apply_state,
                                &ready_receipt,
                                outgoing_receipts,
                                validator_proposals,
                                stats,
                                epoch_info_provider,
                            )
                            .map(Some);
                    } else {
                        // There is still some pending data for the receipt, so we update the
                        // pending data count in the state.
                        set(
                            state_update,
                            TrieKey::PendingDataCount {
                                receiver_id: account_id.clone(),
                                receipt_id,
                            },
                            &(pending_data_count.checked_sub(1).ok_or_else(|| {
                                StorageError::StorageInconsistentState(
                                    "pending data count is 0, but there is a new DataReceipt"
                                        .to_string(),
                                )
                            })?),
                        );
                    }
                }
            }
            ReceiptEnum::Action(ref action_receipt) => {
                // Received a new action receipt. We'll first check how many input data items
                // were already received before and saved in the state.
                // And if we have all input data, then we can immediately execute the receipt.
                // If not, then we will postpone this receipt for later.
                let mut pending_data_count: u32 = 0;
                for data_id in &action_receipt.input_data_ids {
                    if get_received_data(state_update, account_id, *data_id)?.is_none() {
                        pending_data_count += 1;
                        // The data for a given data_id is not available, so we save a link to this
                        // receipt_id for the pending data_id into the state.
                        set(
                            state_update,
                            TrieKey::PostponedReceiptId {
                                receiver_id: account_id.clone(),
                                data_id: *data_id,
                            },
                            &receipt.receipt_id,
                        )
                    }
                }
                if pending_data_count == 0 {
                    // All input data is available. Executing the receipt. It will cleanup
                    // input data from the state.
                    return self
                        .apply_action_receipt(
                            state_update,
                            apply_state,
                            receipt,
                            outgoing_receipts,
                            validator_proposals,
                            stats,
                            epoch_info_provider,
                        )
                        .map(Some);
                } else {
                    // Not all input data is available now.
                    // Save the counter for the number of pending input data items into the state.
                    set(
                        state_update,
                        TrieKey::PendingDataCount {
                            receiver_id: account_id.clone(),
                            receipt_id: receipt.receipt_id,
                        },
                        &pending_data_count,
                    );
                    // Save the receipt itself into the state.
                    set_postponed_receipt(state_update, &receipt);
                }
            }
        };
        // We didn't trigger execution, so we need to commit the state.
        state_update
            .commit(StateChangeCause::PostponedReceipt { receipt_hash: receipt.get_hash() });
        Ok(None)
    }

    /// Iterates over the validators in the current shard and updates their accounts to return stake
    /// and allocate rewards. Also updates protocol treasure account if it belongs to the current
    /// shard.
    fn update_validator_accounts(
        &self,
        state_update: &mut TrieUpdate,
        validator_accounts_update: &ValidatorAccountsUpdate,
        stats: &mut ApplyStats,
    ) -> Result<(), RuntimeError> {
        for (account_id, max_of_stakes) in &validator_accounts_update.stake_info {
            if let Some(mut account) = get_account(state_update, account_id)? {
                if let Some(reward) = validator_accounts_update.validator_rewards.get(account_id) {
                    debug!(target: "runtime", "account {} adding reward {} to stake {}", account_id, reward, account.locked());
                    account.set_locked(
                        account
                            .locked()
                            .checked_add(*reward)
                            .ok_or_else(|| RuntimeError::UnexpectedIntegerOverflow)?,
                    );
                }

                debug!(target: "runtime",
                       "account {} stake {} max_of_stakes: {}",
                       account_id, account.locked(), max_of_stakes
                );
                if account.locked() < *max_of_stakes {
                    return Err(StorageError::StorageInconsistentState(format!(
                        "FATAL: staking invariant does not hold. \
                         Account stake {} is less than maximum of stakes {} in the past three epochs",
                        account.locked(),
                        max_of_stakes)).into());
                }
                let last_proposal =
                    *validator_accounts_update.last_proposals.get(account_id).unwrap_or(&0);
                let return_stake = account
                    .locked()
                    .checked_sub(max(*max_of_stakes, last_proposal))
                    .ok_or_else(|| RuntimeError::UnexpectedIntegerOverflow)?;
                debug!(target: "runtime", "account {} return stake {}", account_id, return_stake);
                account.set_locked(
                    account
                        .locked()
                        .checked_sub(return_stake)
                        .ok_or_else(|| RuntimeError::UnexpectedIntegerOverflow)?,
                );
                account.set_amount(
                    account
                        .amount()
                        .checked_add(return_stake)
                        .ok_or_else(|| RuntimeError::UnexpectedIntegerOverflow)?,
                );

                set_account(state_update, account_id.clone(), &account);
            } else if *max_of_stakes > 0 {
                // if max_of_stakes > 0, it means that the account must have locked balance
                // and therefore must exist
                return Err(StorageError::StorageInconsistentState(format!(
                    "Account {} with max of stakes {} is not found",
                    account_id, max_of_stakes
                ))
                .into());
            }
        }

        for (account_id, stake) in validator_accounts_update.slashing_info.iter() {
            if let Some(mut account) = get_account(state_update, &account_id)? {
                let amount_to_slash = stake.unwrap_or(account.locked());
                debug!(target: "runtime", "slashing {} of {} from {}", amount_to_slash, account.locked(), account_id);
                if account.locked() < amount_to_slash {
                    return Err(StorageError::StorageInconsistentState(format!(
                        "FATAL: staking invariant does not hold. Account locked {} is less than slashed {}",
                        account.locked(), amount_to_slash)).into());
                }
                stats.slashed_burnt_amount = stats
                    .slashed_burnt_amount
                    .checked_add(amount_to_slash)
                    .ok_or_else(|| RuntimeError::UnexpectedIntegerOverflow)?;
                account.set_locked(
                    account
                        .locked()
                        .checked_sub(amount_to_slash)
                        .ok_or_else(|| RuntimeError::UnexpectedIntegerOverflow)?,
                );
                set_account(state_update, account_id.clone(), &account);
            } else {
                return Err(StorageError::StorageInconsistentState(format!(
                    "Account {} to slash is not found",
                    account_id
                ))
                .into());
            }
        }

        if let Some(account_id) = &validator_accounts_update.protocol_treasury_account_id {
            // If protocol treasury stakes, then the rewards was already distributed above.
            if !validator_accounts_update.stake_info.contains_key(account_id) {
                let mut account = get_account(state_update, account_id)?.ok_or_else(|| {
                    StorageError::StorageInconsistentState(format!(
                        "Protocol treasury account {} is not found",
                        account_id
                    ))
                })?;
                let treasury_reward = *validator_accounts_update
                    .validator_rewards
                    .get(account_id)
                    .ok_or_else(|| {
                        StorageError::StorageInconsistentState(format!(
                            "Validator reward for the protocol treasury account {} is not found",
                            account_id
                        ))
                    })?;
                account.set_amount(
                    account
                        .amount()
                        .checked_add(treasury_reward)
                        .ok_or_else(|| RuntimeError::UnexpectedIntegerOverflow)?,
                );
                set_account(state_update, account_id.clone(), &account);
            }
        }
        state_update.commit(StateChangeCause::ValidatorAccountsUpdate);

        Ok(())
    }

    pub fn apply_migrations(
        &self,
        state_update: &mut TrieUpdate,
        migration_data: &Arc<MigrationData>,
        migration_flags: &MigrationFlags,
        protocol_version: ProtocolVersion,
    ) -> Result<(Gas, Vec<Receipt>), StorageError> {
        let mut gas_used: Gas = 0;
        if ProtocolFeature::FixStorageUsage.protocol_version() == protocol_version
            && migration_flags.is_first_block_of_version
        {
            for (account_id, delta) in &migration_data.storage_usage_delta {
                match get_account(state_update, account_id)? {
                    Some(mut account) => {
                        // Storage usage is saved in state, hence it is nowhere close to max value
                        // of u64, and maximal delta is 4196, se we can add here without checking
                        // for overflow
                        account.set_storage_usage(account.storage_usage() + delta);
                        set_account(state_update, account_id.clone(), &account);
                    }
                    // Account could have been deleted in the meantime
                    None => {}
                }
            }
            gas_used += migration_data.storage_usage_fix_gas;
            state_update.commit(StateChangeCause::Migration);
        }

        // Re-introduce receipts lost because of a bug in apply_chunks.
        // We take the first block with existing chunk in the first epoch in which protocol feature
        // RestoreReceiptsAfterFix was enabled, and put the restored receipts there.
        // See https://github.com/near/nearcore/pull/4248/ for more details.
        #[cfg(not(feature = "protocol_feature_restore_receipts_after_fix"))]
        let receipts_to_restore = vec![];
        #[cfg(feature = "protocol_feature_restore_receipts_after_fix")]
        let receipts_to_restore = if ProtocolFeature::RestoreReceiptsAfterFix.protocol_version()
            == protocol_version
            && migration_flags.is_first_block_with_chunk_of_version
        {
            // Note that receipts are restored only on mainnet so restored_receipts will be empty on
            // other chains.
            migration_data.restored_receipts.get(&0u64).cloned().unwrap_or_default()
        } else {
            vec![]
        };

        Ok((gas_used, receipts_to_restore))
    }

    /// Applies new singed transactions and incoming receipts for some chunk/shard on top of
    /// given trie and the given state root.
    /// If the validator accounts update is provided, updates validators accounts.
    /// All new signed transactions should be valid and already verified by the chunk producer.
    /// If any transaction is invalid, it would return an `InvalidTxError`.
    /// Returns an `ApplyResult` that contains the new state root, trie changes,
    /// new outgoing receipts, execution outcomes for
    /// all transactions, local action receipts (generated from transactions with signer ==
    /// receivers) and incoming action receipts.
    pub fn apply(
        &self,
        trie: Trie,
        root: CryptoHash,
        validator_accounts_update: &Option<ValidatorAccountsUpdate>,
        apply_state: &ApplyState,
        incoming_receipts: &[Receipt],
        transactions: &[SignedTransaction],
        epoch_info_provider: &dyn EpochInfoProvider,
        states_to_patch: Option<Vec<StateRecord>>,
    ) -> Result<ApplyResult, RuntimeError> {
        let _span = tracing::debug_span!(target: "runtime", "Runtime::apply").entered();

        if states_to_patch.is_some() && !cfg!(feature = "sandbox") {
            panic!("Can only patch state in sandbox mode");
        }

        let trie = Rc::new(trie);
        let initial_state = TrieUpdate::new(trie.clone(), root);
        let mut state_update = TrieUpdate::new(trie.clone(), root);

        let mut stats = ApplyStats::default();

        if let Some(validator_accounts_update) = validator_accounts_update {
            self.update_validator_accounts(
                &mut state_update,
                validator_accounts_update,
                &mut stats,
            )?;
        }

        let (gas_used_for_migrations, mut receipts_to_restore) = self
            .apply_migrations(
                &mut state_update,
                &apply_state.migration_data,
                &apply_state.migration_flags,
                apply_state.current_protocol_version,
            )
            .map_err(|e| RuntimeError::StorageError(e))?;
        // If we have receipts that need to be restored, prepend them to the list of incoming receipts
        let incoming_receipts = if receipts_to_restore.is_empty() {
            incoming_receipts
        } else {
            receipts_to_restore.extend_from_slice(incoming_receipts);
            receipts_to_restore.as_slice()
        };

        if !apply_state.is_new_chunk
            && apply_state.current_protocol_version
                >= ProtocolFeature::FixApplyChunks.protocol_version()
        {
            let (trie_changes, state_changes) = state_update.finalize()?;
            let proof = trie.recorded_storage();
            return Ok(ApplyResult {
                state_root: trie_changes.new_root,
                trie_changes,
                validator_proposals: vec![],
                outgoing_receipts: vec![],
                outcomes: vec![],
                state_changes,
                stats,
                proof,
            });
        }

        let mut outgoing_receipts = Vec::new();
        let mut validator_proposals = vec![];
        let mut local_receipts = vec![];
        let mut outcomes = vec![];
        // This contains the gas "burnt" for refund receipts. Even though we don't actually
        // charge any gas for refund receipts, we still count the gas use towards the block gas
        // limit
        let mut total_gas_burnt = gas_used_for_migrations;

        for signed_transaction in transactions {
            let (receipt, outcome_with_id) = self.process_transaction(
                &mut state_update,
                apply_state,
                signed_transaction,
                &mut stats,
            )?;
            if receipt.receiver_id == signed_transaction.transaction.signer_id {
                local_receipts.push(receipt);
            } else {
                outgoing_receipts.push(receipt);
            }

            total_gas_burnt += outcome_with_id.outcome.gas_burnt;

            outcomes.push(outcome_with_id);
        }

        let mut delayed_receipts_indices: DelayedReceiptIndices =
            get(&state_update, &TrieKey::DelayedReceiptIndices)?.unwrap_or_default();
        let initial_delayed_receipt_indices = delayed_receipts_indices.clone();

        let mut process_receipt = |receipt: &Receipt,
                                   state_update: &mut TrieUpdate,
                                   total_gas_burnt: &mut Gas|
         -> Result<_, RuntimeError> {
            self.process_receipt(
                state_update,
                apply_state,
                receipt,
                &mut outgoing_receipts,
                &mut validator_proposals,
                &mut stats,
                epoch_info_provider,
            )?
            .into_iter()
            .try_for_each(
                |outcome_with_id: ExecutionOutcomeWithId| -> Result<(), RuntimeError> {
                    *total_gas_burnt =
                        safe_add_gas(*total_gas_burnt, outcome_with_id.outcome.gas_burnt)?;
                    outcomes.push(outcome_with_id);
                    Ok(())
                },
            )?;
            Ok(())
        };

        let gas_limit = apply_state.gas_limit.unwrap_or(Gas::max_value());

        // We first process local receipts. They contain staking, local contract calls, etc.
        for receipt in local_receipts.iter() {
            if total_gas_burnt < gas_limit {
                // NOTE: We don't need to validate the local receipt, because it's just validated in
                // the `verify_and_charge_transaction`.
                process_receipt(&receipt, &mut state_update, &mut total_gas_burnt)?;
            } else {
                Self::delay_receipt(&mut state_update, &mut delayed_receipts_indices, receipt)?;
            }
        }

        // Then we process the delayed receipts. It's a backlog of receipts from the past blocks.
        while delayed_receipts_indices.first_index < delayed_receipts_indices.next_available_index {
            if total_gas_burnt >= gas_limit {
                break;
            }
            let key = TrieKey::DelayedReceipt { index: delayed_receipts_indices.first_index };
            let receipt: Receipt = get(&state_update, &key)?.ok_or_else(|| {
                StorageError::StorageInconsistentState(format!(
                    "Delayed receipt #{} should be in the state",
                    delayed_receipts_indices.first_index
                ))
            })?;

            // Validating the delayed receipt. If it fails, it's likely the state is inconsistent.
            validate_receipt(&apply_state.config.wasm_config.limit_config, &receipt).map_err(
                |e| {
                    StorageError::StorageInconsistentState(format!(
                        "Delayed receipt #{} in the state is invalid: {}",
                        delayed_receipts_indices.first_index, e
                    ))
                },
            )?;

            state_update.remove(key);
            // Math checked above: first_index is less than next_available_index
            delayed_receipts_indices.first_index += 1;
            process_receipt(&receipt, &mut state_update, &mut total_gas_burnt)?;
        }

        // And then we process the new incoming receipts. These are receipts from other shards.
        for receipt in incoming_receipts.iter() {
            // Validating new incoming no matter whether we have available gas or not. We don't
            // want to store invalid receipts in state as delayed.
            validate_receipt(&apply_state.config.wasm_config.limit_config, &receipt)
                .map_err(RuntimeError::ReceiptValidationError)?;
            if total_gas_burnt < gas_limit {
                process_receipt(&receipt, &mut state_update, &mut total_gas_burnt)?;
            } else {
                Self::delay_receipt(&mut state_update, &mut delayed_receipts_indices, receipt)?;
            }
        }

        if delayed_receipts_indices != initial_delayed_receipt_indices {
            set(&mut state_update, TrieKey::DelayedReceiptIndices, &delayed_receipts_indices);
        }

        check_balance(
            &apply_state.config.transaction_costs,
            &initial_state,
            &state_update,
            validator_accounts_update,
            incoming_receipts,
            transactions,
            &outgoing_receipts,
            &stats,
            apply_state.current_protocol_version,
        )?;

        state_update.commit(StateChangeCause::UpdatedDelayedReceipts);

        #[cfg(feature = "sandbox")]
        if let Some(patch) = states_to_patch {
            self.apply_state_patches(&mut state_update, patch);
        }

        let (trie_changes, state_changes) = state_update.finalize()?;

        // Dedup proposals from the same account.
        // The order is deterministically changed.
        let mut unique_proposals = vec![];
        let mut account_ids = HashSet::new();
        for proposal in validator_proposals.into_iter().rev() {
            let account_id = proposal.account_id();
            if !account_ids.contains(account_id) {
                account_ids.insert(account_id.clone());
                unique_proposals.push(proposal);
            }
        }

        let state_root = trie_changes.new_root;
        let proof = trie.recorded_storage();
        Ok(ApplyResult {
            state_root,
            trie_changes,
            validator_proposals: unique_proposals,
            outgoing_receipts,
            outcomes,
            state_changes,
            stats,
            proof,
        })
    }

    // Adds the given receipt into the end of the delayed receipt queue in the state.
    pub fn delay_receipt(
        state_update: &mut TrieUpdate,
        delayed_receipts_indices: &mut DelayedReceiptIndices,
        receipt: &Receipt,
    ) -> Result<(), StorageError> {
        set(
            state_update,
            TrieKey::DelayedReceipt { index: delayed_receipts_indices.next_available_index },
            receipt,
        );
        delayed_receipts_indices.next_available_index =
            delayed_receipts_indices.next_available_index.checked_add(1).ok_or_else(|| {
                StorageError::StorageInconsistentState(
                    "Next available index for delayed receipt exceeded the integer limit"
                        .to_string(),
                )
            })?;
        Ok(())
    }

    #[cfg(feature = "sandbox")]
    fn apply_state_patches(
        &self,
        state_update: &mut TrieUpdate,
        states_to_patch: Vec<StateRecord>,
    ) {
        for record in states_to_patch {
            match record {
                StateRecord::Account { account_id, account } => {
                    set_account(state_update, account_id, &account);
                }
                StateRecord::Data { account_id, data_key, value } => {
                    state_update.set(TrieKey::ContractData { key: data_key, account_id }, value);
                }
                StateRecord::Contract { account_id, code } => {
                    let acc = get_account(&state_update, &account_id).expect("Failed to read state").expect("Code state record should be preceded by the corresponding account record");
                    // Recompute contract code hash.
                    let code = ContractCode::new(code, None);
                    set_code(state_update, account_id, &code);
                    assert_eq!(code.get_hash(), acc.code_hash());
                }
                StateRecord::AccessKey { account_id, public_key, access_key } => {
                    set_access_key(state_update, account_id, public_key, &access_key);
                }
                _ => unimplemented!("patch_state can only patch Account, AccessKey, Contract and Data kind of StateRecord")
            }
        }
        state_update.commit(StateChangeCause::Migration);
    }

    /// It's okay to use unsafe math here, because this method should only be called on the trusted
    /// state records (e.g. at launch from genesis)
    pub fn compute_storage_usage(
        &self,
        records: &[StateRecord],
        config: &RuntimeConfig,
    ) -> HashMap<AccountId, u64> {
        let mut storage_computer = StorageComputer::new(config);
        storage_computer.process_records(records);
        storage_computer.finalize()
    }

    /// Balances are account, publickey, initial_balance, initial_tx_stake
    pub fn apply_genesis_state(
        &self,
        tries: ShardTries,
        shard_id: ShardId,
        validators: &[(AccountId, PublicKey, Balance)],
        genesis: &Genesis,
        config: &RuntimeConfig,
        shard_account_ids: HashSet<AccountId>,
    ) -> StateRoot {
        GenesisStateApplier::apply(tries, shard_id, validators, config, genesis, shard_account_ids)
    }
}
