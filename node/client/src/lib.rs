#[macro_use]
extern crate log;
extern crate beacon;
extern crate chain;
extern crate node_runtime;
extern crate parking_lot;
extern crate primitives;
extern crate serde;

pub mod test_utils;

use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::{cmp, env, fs};

use env_logger::Builder;
use parking_lot::RwLock;

use beacon::types::{BeaconBlockChain, SignedBeaconBlock, SignedBeaconBlockHeader};
use chain::SignedBlock;
use primitives::hash::CryptoHash;
use primitives::signer::InMemorySigner;
use primitives::traits::Signer;
use primitives::types::{AccountId, BlockId, ConsensusBlockBody, UID, AuthorityStake};
use transaction::ChainPayload;
use shard::{ShardBlockChain, SignedShardBlock};
use node_runtime::ApplyState;
use storage::Storage;
use configs::ClientConfig;

pub struct Client {
    pub account_id: AccountId,
    pub signer: InMemorySigner,

    pub shard_chain: Arc<ShardBlockChain>,
    pub beacon_chain: BeaconBlockChain,

    // TODO: The following logic might need to be hidden somewhere.
    /// Stores blocks that cannot be added yet.
    pending_beacon_blocks: RwLock<HashMap<CryptoHash, SignedBeaconBlock>>,
    pending_shard_blocks: RwLock<HashMap<CryptoHash, SignedShardBlock>>,
}

fn configure_logging(log_level: log::LevelFilter) {
    let internal_targets =
        vec!["consensus", "near-rpc", "network", "producer", "runtime", "service", "wasm"];
    let mut builder = Builder::from_default_env();
    internal_targets.iter().for_each(|internal_targets| {
        builder.filter(Some(internal_targets), log_level);
    });

    let other_log_level = cmp::min(log_level, log::LevelFilter::Info);
    builder.filter(None, other_log_level);

    if let Ok(lvl) = env::var("RUST_LOG") {
        builder.parse(&lvl);
    }
    builder.init();
}

pub const DEFAULT_BASE_PATH: &str = ".";
pub const DEFAULT_LOG_LEVEL: log::LevelFilter = log::LevelFilter::Info;


const STORAGE_PATH: &str = "storage/db";
const KEY_STORE_PATH: &str = "storage/keystore";

fn get_storage(base_path: &Path) -> Arc<Storage> {
    let mut storage_path = base_path.to_owned();
    storage_path.push(STORAGE_PATH);
    match fs::canonicalize(storage_path.clone()) {
        Ok(path) => info!("Opening storage database at {:?}", path),
        _ => info!("Could not resolve {:?} path", storage_path),
    };
    Arc::new(storage::open_database(&storage_path.to_string_lossy()))
}

pub type ChainConsensusBlockBody = ConsensusBlockBody<ChainPayload>;

impl Client {
    pub fn new(config: &ClientConfig) -> Self {
        let storage = get_storage(&config.base_path);

        let chain_spec = &config.chain_spec;
        let shard_chain = Arc::new(ShardBlockChain::new(chain_spec, storage.clone()));
        let genesis = SignedBeaconBlock::genesis(shard_chain.chain.genesis_hash);
        let beacon_chain = BeaconBlockChain::new(genesis, &chain_spec, storage.clone());

        let mut key_file_path = config.base_path.to_path_buf();
        key_file_path.push(KEY_STORE_PATH);
        let signer = InMemorySigner::from_key_file(
            config.account_id.clone(),
            key_file_path.as_path(),
            config.public_key.clone(),
        );

        configure_logging(config.log_level);

        Self {
            account_id: config.account_id.clone(),
            signer,
            shard_chain,
            beacon_chain,
            pending_beacon_blocks: RwLock::new(HashMap::new()),
            pending_shard_blocks: RwLock::new(HashMap::new()),
        }
    }

    // Block producer code.
    pub fn produce_block(
        &self,
        body: ChainConsensusBlockBody,
    ) -> (SignedBeaconBlock, SignedShardBlock) {
        // TODO: verify signature
        let transactions =
            body.messages.into_iter().flat_map(|message| message.body.payload.body).collect();

        let last_block = self.beacon_chain.chain.best_block();
        let last_shard_block = self
            .shard_chain
            .chain
            .get_block(&BlockId::Hash(last_block.body.header.shard_block_hash))
            .expect("At the moment we should have shard blocks accompany beacon blocks");
        let authorities = self
            .beacon_chain
            .authority
            .read()
            .get_authorities(last_block.body.header.index)
            .expect("Authorities should be present for given block to produce it");
        let shard_id = last_shard_block.body.header.shard_id;
        let apply_state = ApplyState {
            root: last_shard_block.body.header.merkle_root_state,
            parent_block_hash: last_block.block_hash(),
            block_index: last_block.body.header.index + 1,
            shard_id,
        };
        let apply_result = self.shard_chain.runtime.write().apply(
            &apply_state,
            &[],
            transactions,
        );
        self.shard_chain.state_db.commit(apply_result.transaction).ok();
        let mut shard_block = SignedShardBlock::new(
            shard_id,
            last_shard_block.body.header.index + 1,
            last_shard_block.block_hash(),
            apply_result.root,
            apply_result.filtered_transactions,
            apply_result.new_receipts,
        );
        let mut block = SignedBeaconBlock::new(
            last_block.body.header.index + 1,
            last_block.block_hash(),
            apply_result.authority_proposals,
            shard_block.block_hash(),
        );
        let authority_mask: Vec<bool> =
            authorities.iter().map(|a| a.account_id == self.signer.account_id()).collect();
        let signature = shard_block.sign(&self.signer);
        shard_block.add_signature(signature);
        shard_block.authority_mask = authority_mask.clone();
        let signature = block.sign(&self.signer);
        block.add_signature(signature);
        block.authority_mask = authority_mask;
        self.shard_chain.insert_block(&shard_block.clone());
        self.beacon_chain.chain.insert_block(block.clone());
        info!(target: "block_producer", "Block body: {:?}", block.body);
        info!(target: "block_producer", "Shard block body: {:?}", shard_block.body);
        io::stdout().flush().expect("Could not flush stdout");

        // Update the authority.
        self.update_authority(&block.header());
        (block, shard_block)
    }

    // Block importer code.
    fn add_block(&self, beacon_block: SignedBeaconBlock, shard_block: &SignedShardBlock) {
        let parent_hash = beacon_block.body.header.parent_hash;
        let parent_shard_hash = shard_block.body.header.parent_hash;
        // we can unwrap because parent is guaranteed to exist
        let prev_header = self
            .beacon_chain
            .chain
            .get_header(&BlockId::Hash(parent_hash))
            .expect("Parent is known but header not found.");
        let prev_shard_block = self
            .shard_chain
            .chain
            .get_block(&BlockId::Hash(parent_shard_hash))
            .expect("At this moment shard chain should be present together with beacon chain");
        let prev_shard_header = prev_shard_block.header();
        let apply_state = ApplyState {
            root: prev_shard_header.body.merkle_root_state,
            block_index: prev_header.body.index,
            parent_block_hash: parent_hash,
            shard_id: shard_block.body.header.shard_id,
        };
        let apply_result = self.shard_chain.runtime.write().check(
            &apply_state,
            &prev_shard_block.body.new_receipts,
            &shard_block.body.transactions,
        );
        match apply_result {
            Some((db_transaction, root)) => {
                if root != prev_shard_header.body.merkle_root_state {
                    info!(
                        "Merkle root {} is not equal to received {} after applying the transactions from {:?}",
                        prev_shard_header.body.merkle_root_state,
                        root,
                        beacon_block
                    );
                    return;
                }
                self.shard_chain.state_db.commit(db_transaction).ok();
                self.shard_chain.insert_block(&shard_block);
                self.beacon_chain.chain.insert_block(beacon_block);
            }
            None => {
                info!("Found incorrect transaction in block {:?}", beacon_block);
                return;
            }
        }
    }

    fn blocks_to_process(
        &self,
    ) -> (Vec<SignedBeaconBlock>, HashMap<CryptoHash, SignedBeaconBlock>) {
        let mut part_add = vec![];
        let mut part_pending = HashMap::default();
        for (hash, other) in self.pending_beacon_blocks.write().drain() {
            if self.beacon_chain.chain.is_known(&other.body.header.parent_hash)
                && (self.shard_chain.chain.is_known(&other.body.header.shard_block_hash)
                    || self
                        .pending_shard_blocks
                        .read()
                        .contains_key(&other.body.header.shard_block_hash))
            {
                part_add.push(other);
            } else {
                part_pending.insert(hash, other);
            }
        }
        (part_add, part_pending)
    }

    /// Attempts to import a beacon block. Fails to import if there are no known parent blocks.
    /// If succeeds might unlock more blocks that were waiting for this parent. If import changes
    /// the best block then it returns it, otherwise it returns None.
    pub fn import_beacon_block(
        &self,
        beacon_block: SignedBeaconBlock,
    ) -> Option<SignedBeaconBlock> {
        // Check if this block was either already added, or it is already pending, or it has
        // invalid signature.
        let hash = beacon_block.block_hash();
        if self.beacon_chain.chain.is_known(&hash)
            || self.pending_beacon_blocks.write().contains_key(&hash)
        {
            return None;
        }
        self.pending_beacon_blocks.write().insert(hash, beacon_block);
        let best_block_hash = self.beacon_chain.chain.best_hash();

        let mut blocks_to_add: Vec<SignedBeaconBlock> = vec![];
        // Loop until we run out of blocks to add.
        loop {
            // Only keep those blocks in `pending_blocks` that are still pending.
            // Otherwise put it in `blocks_to_add`.
            let (part_add, part_pending) = self.blocks_to_process();
            blocks_to_add.extend(part_add);
            *self.pending_beacon_blocks.write() = part_pending;

            // Get the next block to add, unless there are no more blocks left.
            let next_beacon_block = match blocks_to_add.pop() {
                Some(b) => b,
                None => break,
            };
            let hash = next_beacon_block.block_hash();
            if self.beacon_chain.chain.is_known(&hash) {
                continue;
            }

            let next_shard_block = self
                .pending_shard_blocks
                .write()
                .remove(&next_beacon_block.body.header.shard_block_hash)
                .expect("Expected to have shard block present when processing beacon block");

            self.add_block(next_beacon_block.clone(), &next_shard_block);
            // Update the authority.
            self.update_authority(&next_beacon_block.header());
        }
        let new_best_block = self.beacon_chain.chain.best_block();
        if new_best_block.block_hash() == best_block_hash {
            None
        } else {
            Some(new_best_block)
        }
    }

    // Authority-related code. Consider hiding it inside the shard chain.
    fn update_authority(&self, beacon_header: &SignedBeaconBlockHeader) {
        self.beacon_chain.authority.write().process_block_header(beacon_header);
    }

    /// Returns own UID and UID to authority map for the given block number.
    /// If the owner is not participating in the block then it returns None.
    pub fn get_uid_to_authority_map(
        &self,
        block_index: u64,
    ) -> (Option<UID>, HashMap<UID, AuthorityStake>) {
        let next_authorities = self
            .beacon_chain
            .authority
            .read()
            .get_authorities(block_index)
            .unwrap_or_else(|_| panic!("Failed to get authorities for block index {}", block_index));

        let mut uid_to_authority_map = HashMap::new();
        let mut owner_uid = None;
        for (index, authority) in next_authorities.into_iter().enumerate() {
            if authority.account_id == self.account_id {
                owner_uid = Some(index as UID);
            }
            uid_to_authority_map.insert(index as UID, authority);
        }
        (owner_uid, uid_to_authority_map)
    }
}
