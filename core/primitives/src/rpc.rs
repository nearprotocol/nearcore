use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::account::{AccessKey, AccessKeyPermission, Account, FunctionCallPermission};
use crate::block::{Block, BlockHeader, BlockHeaderInner};
use crate::crypto::signature::{PublicKey, SecretKey, Signature};
use crate::hash::CryptoHash;
use crate::logging;
use crate::serialize::{from_base, option_u128_dec_format, to_base, to_base64, u128_dec_format};
use crate::transaction::{Action, SignedTransaction, TransactionLog, TransactionResult};
use crate::types::{
    AccountId, Balance, BlockIndex, Gas, Nonce, StorageUsage, ValidatorStake, Version,
};

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct PublicKeyView(Vec<u8>);

impl Serialize for PublicKeyView {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&to_base(&self.0))
    }
}

impl<'de> Deserialize<'de> for PublicKeyView {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        from_base(&s)
            .map(|v| PublicKeyView(v))
            .map_err(|err| serde::de::Error::custom(err.to_string()))
    }
}

impl fmt::Display for PublicKeyView {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", to_base(&self.0))
    }
}

impl From<PublicKey> for PublicKeyView {
    fn from(public_key: PublicKey) -> Self {
        Self(public_key.0.as_ref().to_vec())
    }
}

impl From<PublicKeyView> for PublicKey {
    fn from(view: PublicKeyView) -> Self {
        Self::try_from(view.0).expect("Failed to get PublicKey from PublicKeyView")
    }
}

#[derive(Debug)]
pub struct SecretKeyView(Vec<u8>);

impl Serialize for SecretKeyView {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&to_base(&self.0))
    }
}

impl<'de> Deserialize<'de> for SecretKeyView {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        from_base(&s)
            .map(|v| SecretKeyView(v))
            .map_err(|err| serde::de::Error::custom(err.to_string()))
    }
}

impl From<SecretKey> for SecretKeyView {
    fn from(secret_key: SecretKey) -> Self {
        Self(secret_key.0[..].to_vec())
    }
}

impl From<SecretKeyView> for SecretKey {
    fn from(view: SecretKeyView) -> Self {
        TryFrom::<&[u8]>::try_from(view.0.as_ref())
            .expect("Failed to get SecretKeyView from SecretKey")
    }
}

#[derive(Debug, Clone)]
pub struct SignatureView(Vec<u8>);

impl Serialize for SignatureView {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&to_base(&self.0))
    }
}

impl<'de> Deserialize<'de> for SignatureView {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        from_base(&s)
            .map(|v| SignatureView(v))
            .map_err(|err| serde::de::Error::custom(err.to_string()))
    }
}

impl From<Signature> for SignatureView {
    fn from(signature: Signature) -> Self {
        Self(signature.0.as_ref().to_vec())
    }
}

impl From<SignatureView> for Signature {
    fn from(view: SignatureView) -> Self {
        Signature::try_from(view.0).expect("Failed to get Signature from SignatureView")
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CryptoHashView(Vec<u8>);

impl Serialize for CryptoHashView {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&to_base(&self.0))
    }
}

impl<'de> Deserialize<'de> for CryptoHashView {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        from_base(&s)
            .map(|v| CryptoHashView(v))
            .map_err(|err| serde::de::Error::custom(err.to_string()))
    }
}

impl From<CryptoHash> for CryptoHashView {
    fn from(hash: CryptoHash) -> Self {
        CryptoHashView(hash.as_ref().to_vec())
    }
}

impl From<CryptoHashView> for CryptoHash {
    fn from(view: CryptoHashView) -> Self {
        CryptoHash::try_from(view.0).expect("Failed to convert CryptoHashView to CryptoHash")
    }
}

/// A view of the account
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub struct AccountView {
    #[serde(with = "u128_dec_format")]
    pub amount: Balance,
    #[serde(with = "u128_dec_format")]
    pub staked: Balance,
    pub code_hash: CryptoHashView,
    pub storage_usage: StorageUsage,
    pub storage_paid_at: BlockIndex,
}

impl From<Account> for AccountView {
    fn from(account: Account) -> Self {
        AccountView {
            amount: account.amount,
            staked: account.staked,
            code_hash: account.code_hash.into(),
            storage_usage: account.storage_usage,
            storage_paid_at: account.storage_paid_at,
        }
    }
}

impl From<AccountView> for Account {
    fn from(view: AccountView) -> Self {
        Self {
            amount: view.amount,
            staked: view.staked,
            code_hash: view.code_hash.into(),
            storage_usage: view.storage_usage,
            storage_paid_at: view.storage_paid_at,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub enum AccessKeyPermissionView {
    FunctionCall {
        #[serde(with = "option_u128_dec_format")]
        allowance: Option<Balance>,
        receiver_id: AccountId,
        method_names: Vec<String>,
    },
    FullAccess,
}

impl From<AccessKeyPermission> for AccessKeyPermissionView {
    fn from(permission: AccessKeyPermission) -> Self {
        match permission {
            AccessKeyPermission::FunctionCall(func_call) => AccessKeyPermissionView::FunctionCall {
                allowance: func_call.allowance,
                receiver_id: func_call.receiver_id,
                method_names: func_call.method_names,
            },
            AccessKeyPermission::FullAccess => AccessKeyPermissionView::FullAccess,
        }
    }
}

impl From<AccessKeyPermissionView> for AccessKeyPermission {
    fn from(view: AccessKeyPermissionView) -> Self {
        match view {
            AccessKeyPermissionView::FunctionCall { allowance, receiver_id, method_names } => {
                AccessKeyPermission::FunctionCall(FunctionCallPermission {
                    allowance,
                    receiver_id,
                    method_names,
                })
            }
            AccessKeyPermissionView::FullAccess => AccessKeyPermission::FullAccess,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub struct AccessKeyView {
    pub nonce: Nonce,
    pub permission: AccessKeyPermissionView,
}

impl From<AccessKey> for AccessKeyView {
    fn from(access_key: AccessKey) -> Self {
        Self { nonce: access_key.nonce, permission: access_key.permission.into() }
    }
}

impl From<AccessKeyView> for AccessKey {
    fn from(view: AccessKeyView) -> Self {
        Self { nonce: view.nonce, permission: view.permission.into() }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ViewStateResult {
    pub values: HashMap<Vec<u8>, Vec<u8>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CallResult {
    pub result: Vec<u8>,
    pub logs: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueryError {
    pub error: String,
    pub logs: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AccessKeyInfo {
    pub public_key: PublicKeyView,
    pub access_key: AccessKeyView,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum QueryResponse {
    ViewAccount(AccountView),
    ViewState(ViewStateResult),
    CallResult(CallResult),
    Error(QueryError),
    AccessKey(Option<AccessKeyView>),
    AccessKeyList(Vec<AccessKeyInfo>),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StatusSyncInfo {
    pub latest_block_hash: CryptoHashView,
    pub latest_block_height: BlockIndex,
    pub latest_state_root: CryptoHashView,
    pub latest_block_time: DateTime<Utc>,
    pub syncing: bool,
}

// TODO: add more information to ValidatorInfo
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct ValidatorInfo {
    pub account_id: AccountId,
    pub is_slashed: bool,
}

// TODO: add more information to status.
#[derive(Serialize, Deserialize, Debug)]
pub struct StatusResponse {
    /// Binary version.
    pub version: Version,
    /// Unique chain id.
    pub chain_id: String,
    /// Address for RPC server.
    pub rpc_addr: String,
    /// Current epoch validators.
    pub validators: Vec<ValidatorInfo>,
    /// Sync status of the node.
    pub sync_info: StatusSyncInfo,
}

impl TryFrom<QueryResponse> for AccountView {
    type Error = String;

    fn try_from(query_response: QueryResponse) -> Result<Self, Self::Error> {
        match query_response {
            QueryResponse::ViewAccount(acc) => Ok(acc),
            _ => Err("Invalid type of response".into()),
        }
    }
}

impl TryFrom<QueryResponse> for ViewStateResult {
    type Error = String;

    fn try_from(query_response: QueryResponse) -> Result<Self, Self::Error> {
        match query_response {
            QueryResponse::ViewState(vs) => Ok(vs),
            _ => Err("Invalid type of response".into()),
        }
    }
}

impl TryFrom<QueryResponse> for Option<AccessKeyView> {
    type Error = String;

    fn try_from(query_response: QueryResponse) -> Result<Self, Self::Error> {
        match query_response {
            QueryResponse::AccessKey(access_key) => Ok(access_key),
            _ => Err("Invalid type of response".into()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockHeaderView {
    pub height: BlockIndex,
    pub epoch_hash: CryptoHash,
    pub prev_hash: CryptoHashView,
    pub prev_state_root: CryptoHashView,
    pub tx_root: CryptoHashView,
    pub timestamp: u64,
    pub approval_mask: Vec<bool>,
    pub approval_sigs: Vec<SignatureView>,
    pub total_weight: u64,
    pub validator_proposals: Vec<ValidatorStake>,
    pub signature: SignatureView,
}

impl From<BlockHeader> for BlockHeaderView {
    fn from(header: BlockHeader) -> Self {
        Self {
            height: header.inner.height,
            epoch_hash: header.inner.epoch_hash,
            prev_hash: header.inner.prev_hash.into(),
            prev_state_root: header.inner.prev_state_root.into(),
            tx_root: header.inner.tx_root.into(),
            timestamp: header.inner.timestamp,
            approval_mask: header.inner.approval_mask,
            approval_sigs: header
                .inner
                .approval_sigs
                .into_iter()
                .map(|signature| signature.into())
                .collect(),
            total_weight: header.inner.total_weight.to_num(),
            validator_proposals: header.inner.validator_proposals,
            signature: header.signature.into(),
        }
    }
}

impl From<BlockHeaderView> for BlockHeader {
    fn from(view: BlockHeaderView) -> Self {
        let mut header = Self {
            inner: BlockHeaderInner {
                height: view.height,
                epoch_hash: view.epoch_hash.into(),
                prev_hash: view.prev_hash.into(),
                prev_state_root: view.prev_state_root.into(),
                tx_root: view.tx_root.into(),
                timestamp: view.timestamp,
                approval_mask: view.approval_mask,
                approval_sigs: view
                    .approval_sigs
                    .into_iter()
                    .map(|signature| signature.into())
                    .collect(),
                total_weight: view.total_weight.into(),
                validator_proposals: view.validator_proposals,
            },
            signature: view.signature.into(),
            hash: CryptoHash::default(),
        };
        header.init();
        header
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BlockView {
    pub header: BlockHeaderView,
    pub transactions: Vec<SignedTransactionView>,
}

impl From<Block> for BlockView {
    fn from(block: Block) -> Self {
        BlockView {
            header: block.header.into(),
            transactions: block.transactions.into_iter().map(|tx| tx.into()).collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ActionView {
    CreateAccount,
    DeployContract {
        code: String,
    },
    FunctionCall {
        method_name: String,
        args: String,
        gas: Gas,
        #[serde(with = "u128_dec_format")]
        deposit: Balance,
    },
    Transfer {
        #[serde(with = "u128_dec_format")]
        deposit: Balance,
    },
    Stake {
        #[serde(with = "u128_dec_format")]
        stake: Balance,
        public_key: PublicKeyView,
    },
    AddKey {
        public_key: PublicKeyView,
        access_key: AccessKeyView,
    },
    DeleteKey {
        public_key: PublicKey,
    },
    DeleteAccount {
        beneficiary_id: AccountId,
    },
}

impl From<Action> for ActionView {
    fn from(action: Action) -> Self {
        match action {
            Action::CreateAccount(_) => ActionView::CreateAccount,
            Action::DeployContract(action) => {
                ActionView::DeployContract { code: to_base64(&action.code) }
            }
            Action::FunctionCall(action) => ActionView::FunctionCall {
                method_name: action.method_name,
                args: to_base(&action.args),
                gas: action.gas,
                deposit: action.deposit,
            },
            Action::Transfer(action) => ActionView::Transfer { deposit: action.deposit },
            Action::Stake(action) => {
                ActionView::Stake { stake: action.stake, public_key: action.public_key.into() }
            }
            Action::AddKey(action) => ActionView::AddKey {
                public_key: action.public_key.into(),
                access_key: action.access_key.into(),
            },
            Action::DeleteKey(action) => {
                ActionView::DeleteKey { public_key: action.public_key.into() }
            }
            Action::DeleteAccount(action) => {
                ActionView::DeleteAccount { beneficiary_id: action.beneficiary_id }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SignedTransactionView {
    signer_id: AccountId,
    public_key: PublicKeyView,
    nonce: Nonce,
    receiver_id: AccountId,
    actions: Vec<ActionView>,
    signature: SignatureView,
    hash: CryptoHashView,
}

impl From<SignedTransaction> for SignedTransactionView {
    fn from(signed_tx: SignedTransaction) -> Self {
        let hash = signed_tx.get_hash().into();
        SignedTransactionView {
            signer_id: signed_tx.transaction.signer_id,
            public_key: signed_tx.transaction.public_key.into(),
            nonce: signed_tx.transaction.nonce,
            receiver_id: signed_tx.transaction.receiver_id,
            actions: signed_tx
                .transaction
                .actions
                .into_iter()
                .map(|action| action.into())
                .collect(),
            signature: signed_tx.signature.into(),
            hash,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub enum FinalTransactionStatus {
    Unknown,
    Started,
    Failed,
    Completed,
}

impl Default for FinalTransactionStatus {
    fn default() -> Self {
        FinalTransactionStatus::Unknown
    }
}

impl FinalTransactionStatus {
    pub fn to_code(&self) -> u64 {
        match self {
            FinalTransactionStatus::Completed => 0,
            FinalTransactionStatus::Failed => 1,
            FinalTransactionStatus::Started => 2,
            FinalTransactionStatus::Unknown => std::u64::MAX,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TransactionLogView {
    pub hash: CryptoHashView,
    pub result: TransactionResult,
}

impl From<TransactionLog> for TransactionLogView {
    fn from(log: TransactionLog) -> Self {
        Self { hash: log.hash.into(), result: log.result }
    }
}

/// Result of transaction and all of subsequent the receipts.
#[derive(Serialize, Deserialize)]
pub struct FinalTransactionResult {
    /// Status of the whole transaction and it's receipts.
    pub status: FinalTransactionStatus,
    /// Transaction results.
    pub transactions: Vec<TransactionLogView>,
}

impl fmt::Debug for FinalTransactionResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FinalTransactionResult")
            .field("status", &self.status)
            .field("transactions", &format_args!("{}", logging::pretty_vec(&self.transactions)))
            .finish()
    }
}

impl FinalTransactionResult {
    pub fn final_log(&self) -> String {
        let mut logs = vec![];
        for transaction in &self.transactions {
            for line in &transaction.result.logs {
                logs.push(line.clone());
            }
        }
        logs.join("\n")
    }

    pub fn last_result(&self) -> Vec<u8> {
        for transaction in self.transactions.iter().rev() {
            if let Some(r) = &transaction.result.result {
                return r.clone();
            }
        }
        vec![]
    }
}
