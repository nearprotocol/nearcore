//! Readonly view of the chain and state of the database.
//! Useful for querying from RPC.

use std::sync::Arc;

use actix::{Actor, Context, Handler};
use log::{error, warn};

use near_chain::{Chain, ChainGenesis, ErrorKind, RuntimeAdapter};
use near_primitives::types::AccountId;
use near_primitives::views::{
    BlockView, ChunkView, EpochValidatorInfo, FinalExecutionOutcomeView, FinalExecutionStatus,
    QueryResponse,
};
use near_store::Store;

use crate::types::{Error, GetBlock, Query, TxStatus};
use crate::{GetChunk, GetValidatorInfo};
use cached::{Cached, SizedCache};
use near_network::types::{NetworkViewClientMessages, NetworkViewClientResponses};
use near_network::{NetworkAdapter, NetworkRequests};
use near_primitives::hash::CryptoHash;
use near_primitives::merkle::verify_path;
use near_primitives::transaction::ExecutionOutcomeWithIdAndProof;

/// Max number of queries that we keep.
const QUERY_REQUEST_LIMIT: usize = 500;

/// View client provides currently committed (to the storage) view of the current chain and state.
pub struct ViewClientActor {
    chain: Chain,
    runtime_adapter: Arc<dyn RuntimeAdapter>,
    network_adapter: Arc<dyn NetworkAdapter>,
    /// Transaction query that needs to be forwarded to other shards
    pub tx_status_requests: SizedCache<CryptoHash, ()>,
    /// Transaction status response
    pub tx_status_response: SizedCache<CryptoHash, FinalExecutionOutcomeView>,
    /// Query requests that need to be forwarded to other shards
    pub query_requests: SizedCache<String, ()>,
    /// Query responses from other nodes (can be errors)
    pub query_responses: SizedCache<String, Result<QueryResponse, String>>,
    /// Receipt outcome requests
    pub receipt_outcome_requests: SizedCache<CryptoHash, ()>,
}

impl ViewClientActor {
    pub fn new(
        store: Arc<Store>,
        chain_genesis: &ChainGenesis,
        runtime_adapter: Arc<dyn RuntimeAdapter>,
        network_adapter: Arc<dyn NetworkAdapter>,
    ) -> Result<Self, Error> {
        // TODO: should we create shared ChainStore that is passed to both Client and ViewClient?
        let chain = Chain::new(store, runtime_adapter.clone(), chain_genesis)?;
        Ok(ViewClientActor {
            chain,
            runtime_adapter,
            network_adapter,
            tx_status_requests: SizedCache::with_size(QUERY_REQUEST_LIMIT),
            tx_status_response: SizedCache::with_size(QUERY_REQUEST_LIMIT),
            query_requests: SizedCache::with_size(QUERY_REQUEST_LIMIT),
            query_responses: SizedCache::with_size(QUERY_REQUEST_LIMIT),
            receipt_outcome_requests: SizedCache::with_size(QUERY_REQUEST_LIMIT),
        })
    }

    fn handle_query(&mut self, msg: Query) -> Result<Option<QueryResponse>, String> {
        if let Some(response) = self.query_responses.cache_remove(&msg.id) {
            self.query_requests.cache_remove(&msg.id);
            return response.map(Some);
        }
        let header = self.chain.head_header().map_err(|e| e.to_string())?.clone();
        let path_parts: Vec<&str> = msg.path.split('/').collect();
        if path_parts.len() <= 1 {
            return Err("Not enough query parameters provided".to_string());
        }
        let account_id = AccountId::from(path_parts[1].clone());
        let shard_id = self.runtime_adapter.account_id_to_shard_id(&account_id);

        // If we have state for the shard that we query return query result directly.
        // Otherwise route query to peers.
        match self.chain.get_chunk_extra(&header.hash, shard_id) {
            Ok(chunk_extra) => {
                let state_root = chunk_extra.state_root;
                self.runtime_adapter
                    .query(
                        &state_root,
                        header.inner_lite.height,
                        header.inner_lite.timestamp,
                        &header.hash,
                        path_parts.clone(),
                        &msg.data,
                    )
                    .map(Some)
                    .map_err(|e| e.to_string())
            }
            Err(e) => {
                match e.kind() {
                    ErrorKind::DBNotFoundErr(_) => {}
                    _ => {
                        warn!(target: "client", "Getting chunk extra failed: {}", e.to_string());
                    }
                }
                // route request
                let validator = self
                    .chain
                    .find_validator_for_forwarding(shard_id)
                    .map_err(|e| e.to_string())?;
                self.query_requests.cache_set(msg.id.clone(), ());
                self.network_adapter.send(NetworkRequests::Query {
                    account_id: validator,
                    path: msg.path.clone(),
                    data: msg.data.clone(),
                    id: msg.id.clone(),
                });
                Ok(None)
            }
        }
    }

    fn get_tx_status(
        &mut self,
        tx_hash: CryptoHash,
        signer_account_id: AccountId,
    ) -> Result<Option<FinalExecutionOutcomeView>, String> {
        if let Some(res) = self.tx_status_response.cache_remove(&tx_hash) {
            self.tx_status_requests.cache_remove(&tx_hash);
            return Ok(Some(res));
        }
        let has_tx_result = match self.chain.get_execution_outcome(&tx_hash) {
            Ok(_) => true,
            Err(e) => match e.kind() {
                ErrorKind::DBNotFoundErr(_) => false,
                _ => {
                    warn!(target: "client", "Error trying to get transaction result: {}", e.to_string());
                    false
                }
            },
        };
        let head = self.chain.head().map_err(|e| e.to_string())?.clone();
        if has_tx_result {
            let tx_result = self.chain.get_final_transaction_result(&tx_hash)?;
            match tx_result.status {
                FinalExecutionStatus::NotStarted | FinalExecutionStatus::Started => {
                    for receipt_view in tx_result.receipts.iter() {
                        let dst_shard_id = *self
                            .chain
                            .get_shard_id_for_receipt_id(&receipt_view.id)
                            .map_err(|e| e.to_string())?;
                        if self.chain.get_chunk_extra(&head.last_block_hash, dst_shard_id).is_err()
                        {
                            let validator = self
                                .chain
                                .find_validator_for_forwarding(dst_shard_id)
                                .map_err(|e| e.to_string())?;
                            self.receipt_outcome_requests.cache_set(receipt_view.id, ());
                            self.network_adapter.send(NetworkRequests::ReceiptOutComeRequest(
                                validator,
                                receipt_view.id,
                            ));
                        }
                    }
                }
                FinalExecutionStatus::SuccessValue(_) | FinalExecutionStatus::Failure(_) => {}
            }
            return Ok(Some(tx_result));
        }
        let target_shard_id = self.runtime_adapter.account_id_to_shard_id(&signer_account_id);
        let validator =
            self.chain.find_validator_for_forwarding(target_shard_id).map_err(|e| e.to_string())?;

        self.tx_status_requests.cache_set(tx_hash, ());
        self.network_adapter.send(NetworkRequests::TxStatus(validator, signer_account_id, tx_hash));
        Ok(None)
    }
}

impl Actor for ViewClientActor {
    type Context = Context<Self>;
}

/// Handles runtime query.
impl Handler<Query> for ViewClientActor {
    type Result = Result<Option<QueryResponse>, String>;

    fn handle(&mut self, msg: Query, _: &mut Context<Self>) -> Self::Result {
        self.handle_query(msg)
    }
}

/// Handles retrieving block from the chain.
impl Handler<GetBlock> for ViewClientActor {
    type Result = Result<BlockView, String>;

    fn handle(&mut self, msg: GetBlock, _: &mut Context<Self>) -> Self::Result {
        match msg {
            GetBlock::Best => match self.chain.head() {
                Ok(head) => self.chain.get_block(&head.last_block_hash).map(Clone::clone),
                Err(err) => Err(err),
            },
            GetBlock::Height(height) => self.chain.get_block_by_height(height).map(Clone::clone),
            GetBlock::Hash(hash) => self.chain.get_block(&hash).map(Clone::clone),
        }
        .map(|block| block.into())
        .map_err(|err| err.to_string())
    }
}

impl Handler<GetChunk> for ViewClientActor {
    type Result = Result<ChunkView, String>;

    fn handle(&mut self, msg: GetChunk, _: &mut Self::Context) -> Self::Result {
        match msg {
            GetChunk::ChunkHash(chunk_hash) => self.chain.get_chunk(&chunk_hash).map(Clone::clone),
            GetChunk::BlockHash(block_hash, shard_id) => {
                self.chain.get_block(&block_hash).map(Clone::clone).and_then(|block| {
                    self.chain
                        .get_chunk(&block.chunks[shard_id as usize].chunk_hash())
                        .map(Clone::clone)
                })
            }
            GetChunk::BlockHeight(block_height, shard_id) => {
                self.chain.get_block_by_height(block_height).map(Clone::clone).and_then(|block| {
                    self.chain
                        .get_chunk(&block.chunks[shard_id as usize].chunk_hash())
                        .map(Clone::clone)
                })
            }
        }
        .map(|chunk| chunk.into())
        .map_err(|err| err.to_string())
    }
}

impl Handler<TxStatus> for ViewClientActor {
    type Result = Result<Option<FinalExecutionOutcomeView>, String>;

    fn handle(&mut self, msg: TxStatus, _: &mut Context<Self>) -> Self::Result {
        self.get_tx_status(msg.tx_hash, msg.signer_account_id)
    }
}

impl Handler<GetValidatorInfo> for ViewClientActor {
    type Result = Result<EpochValidatorInfo, String>;

    fn handle(&mut self, msg: GetValidatorInfo, _: &mut Context<Self>) -> Self::Result {
        self.runtime_adapter.get_validator_info(&msg.last_block_hash).map_err(|e| e.to_string())
    }
}

impl Handler<NetworkViewClientMessages> for ViewClientActor {
    type Result = NetworkViewClientResponses;

    fn handle(&mut self, msg: NetworkViewClientMessages, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            NetworkViewClientMessages::TxStatus { tx_hash, signer_account_id } => {
                if let Ok(Some(result)) = self.get_tx_status(tx_hash, signer_account_id) {
                    NetworkViewClientResponses::TxStatus(result)
                } else {
                    NetworkViewClientResponses::NoResponse
                }
            }
            NetworkViewClientMessages::TxStatusResponse(tx_result) => {
                let tx_hash = tx_result.transaction.id;
                if self.tx_status_requests.cache_remove(&tx_hash).is_some() {
                    self.tx_status_response.cache_set(tx_hash, tx_result);
                }
                NetworkViewClientResponses::NoResponse
            }
            NetworkViewClientMessages::Query { path, data, id } => {
                let query = Query { path, data, id: id.clone() };
                match self.handle_query(query) {
                    Ok(Some(r)) => {
                        NetworkViewClientResponses::QueryResponse { response: Ok(r), id }
                    }
                    Ok(None) => NetworkViewClientResponses::NoResponse,
                    Err(e) => NetworkViewClientResponses::QueryResponse { response: Err(e), id },
                }
            }
            NetworkViewClientMessages::QueryResponse { response, id } => {
                if self.query_requests.cache_get(&id).is_some() {
                    self.query_responses.cache_set(id, response);
                }
                NetworkViewClientResponses::NoResponse
            }
            NetworkViewClientMessages::ReceiptOutcomeRequest(receipt_id) => {
                if let Ok(outcome_with_proof) = self.chain.get_execution_outcome(&receipt_id) {
                    NetworkViewClientResponses::ReceiptOutcomeResponse(
                        ExecutionOutcomeWithIdAndProof {
                            id: receipt_id,
                            outcome_with_proof: outcome_with_proof.clone(),
                        },
                    )
                } else {
                    NetworkViewClientResponses::NoResponse
                }
            }
            NetworkViewClientMessages::ReceiptOutcomeResponse(response) => {
                if self.receipt_outcome_requests.cache_remove(&response.id).is_some() {
                    if let Ok(&shard_id) = self.chain.get_shard_id_for_receipt_id(&response.id) {
                        let block_hash = response.outcome_with_proof.block_hash;
                        if let Ok(Some(&next_block_hash)) =
                            self.chain.get_next_block_hash_with_new_chunk(&block_hash, shard_id)
                        {
                            if let Ok(block) = self.chain.get_block(&next_block_hash) {
                                let ExecutionOutcomeWithIdAndProof { id, outcome_with_proof } =
                                    response;
                                if shard_id < block.chunks.len() as u64 {
                                    if verify_path(
                                        block.chunks[shard_id as usize].inner.outcome_root,
                                        &outcome_with_proof.proof,
                                        &outcome_with_proof.outcome.to_hashes(),
                                    ) {
                                        let mut chain_store_update =
                                            self.chain.mut_store().store_update();
                                        chain_store_update
                                            .save_outcome_with_proof(id, outcome_with_proof);
                                        if let Err(e) = chain_store_update.commit() {
                                            error!(target: "view_client", "Error committing to chain store: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                NetworkViewClientResponses::NoResponse
            }
        }
    }
}
