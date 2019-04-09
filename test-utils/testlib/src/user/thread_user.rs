use std::sync::Arc;
use client::Client;
use primitives::crypto::signer::InMemorySigner;
use crate::user::{NodeUser, POISONED_LOCK_ERR};
use node_runtime::state_viewer::AccountViewCallResult;
use primitives::transaction::SignedTransaction;
use node_http::types::{GetBlocksByIndexRequest, SignedShardBlocksResponse};

pub struct ThreadNodeUser {
    pub client: Arc<Client<InMemorySigner>>,
}

impl ThreadNodeUser {
    pub fn new(client: Arc<Client<InMemorySigner>>) -> ThreadNodeUser {
        ThreadNodeUser { client }
    }
}

impl NodeUser for ThreadNodeUser {
    fn view_account(&self, account_id: &String) -> Result<AccountViewCallResult, String> {
        let mut state_update = self.client.shard_client.get_state_update();
        self.client.shard_client.trie_viewer.view_account(&mut state_update, account_id)
    }

    fn add_transaction(&self, transaction: SignedTransaction) -> Result<(), String> {
        self.client.shard_client.pool.clone().expect("Must have pool").write().expect(POISONED_LOCK_ERR).add_transaction(transaction)
    }

    fn get_account_nonce(&self, account_id: &String) -> Option<u64> {
        self.client.shard_client.get_account_nonce(account_id.clone())
    }

    fn get_best_block_index(&self) -> u64 {
        self.client.beacon_client.chain.best_index()
    }
    fn get_shard_blocks_by_index(
        &self,
        r: GetBlocksByIndexRequest,
    ) -> Result<SignedShardBlocksResponse, String> {
        let start = r.start.unwrap_or_else(|| self.client.shard_client.chain.best_index());
        let limit = r.limit.unwrap_or(25);
        let blocks = self.client.shard_client.chain.get_blocks_by_indices(start, limit);
        Ok(SignedShardBlocksResponse {
            blocks: blocks.into_iter().map(std::convert::Into::into).collect(),
        })
    }
}
