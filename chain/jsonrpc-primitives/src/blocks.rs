use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(thiserror::Error, Debug)]
pub enum RpcBlockError {
    #[error("Block `{0}` is missing")]
    BlockMissing(near_primitives::hash::CryptoHash),
    #[error("Block not found")]
    BlockNotFound(String),
    #[error("There are no fully synchronized blocks yet")]
    NotSyncedYet,
    #[error("The node reached its limits. Try again later. More details: {0}")]
    InternalError(actix::MailboxError),
    #[error("It is a bug if you receive this error type, please, report this incident: https://github.com/near/nearcore/issues/new/choose. Details: {0}")]
    Unreachable(String),
}

#[derive(Serialize, Deserialize)]
pub struct RpcBlockRequest {
    #[serde(flatten)]
    pub block_reference: near_primitives::types::BlockReference,
}

impl From<near_client_primitives::types::GetBlockError> for RpcBlockError {
    fn from(error: near_client_primitives::types::GetBlockError) -> RpcBlockError {
        match error {
            near_client_primitives::types::GetBlockError::BlockMissing(block_hash) => {
                RpcBlockError::BlockMissing(block_hash)
            }
            near_client_primitives::types::GetBlockError::BlockNotFound(s) => {
                RpcBlockError::BlockNotFound(s)
            }
            near_client_primitives::types::GetBlockError::NotSyncedYet => {
                RpcBlockError::NotSyncedYet
            }
            near_client_primitives::types::GetBlockError::IOError(s) => {
                RpcBlockError::Unreachable(s)
            }
            near_client_primitives::types::GetBlockError::Unexpected(s) => {
                near_metrics::inc_counter_vec(
                    &crate::metrics::RPC_UNREACHABLE_ERROR_COUNT,
                    &["RpcBlockError", &s],
                );
                RpcBlockError::Unreachable(s)
            }
        }
    }
}

impl From<actix::MailboxError> for RpcBlockError {
    fn from(error: actix::MailboxError) -> RpcBlockError {
        RpcBlockError::InternalError(error)
    }
}

impl RpcBlockRequest {
    pub fn parse(value: Option<Value>) -> Result<RpcBlockRequest, crate::errors::RpcParseError> {
        let block_reference = if let Ok((block_id,)) =
            crate::utils::parse_params::<(near_primitives::types::BlockId,)>(value.clone())
        {
            near_primitives::types::BlockReference::BlockId(block_id)
        } else {
            crate::utils::parse_params::<near_primitives::types::BlockReference>(value)?
        };
        Ok(RpcBlockRequest { block_reference })
    }
}
