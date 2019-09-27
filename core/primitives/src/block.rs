use std::collections::HashMap;
use std::sync::Arc;

use borsh::{BorshDeserialize, BorshSerialize};
use chrono::prelude::{DateTime, Utc};

use near_crypto::{EmptySigner, KeyType, PublicKey, Signature, Signer};

use crate::hash::{hash, CryptoHash};
use crate::merkle::merklize;
use crate::sharding::{ChunkHashHeight, ShardChunkHeader};
use crate::transaction::SignedTransaction;
use crate::types::{Balance, BlockIndex, EpochId, Gas, MerkleHash, ShardId, ValidatorStake};
use crate::utils::{from_timestamp, to_timestamp};

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Eq, PartialEq)]
pub struct BlockHeaderInner {
    /// Height of this block since the genesis block (height 0).
    pub height: BlockIndex,
    /// Epoch start hash of this block's epoch.
    /// Used for retrieving validator information
    pub epoch_id: EpochId,
    /// Hash of the block previous to this in the chain.
    pub prev_hash: CryptoHash,
    /// Root hash of the state at the previous block.
    pub prev_state_root: MerkleHash,
    /// Root hash of the transactions in the given block.
    pub tx_root: MerkleHash,
    /// Root hash of the chunk receipts in the given block.
    pub chunk_receipts_root: MerkleHash,
    /// Root hash of the chunk headers in the given block.
    pub chunk_headers_root: MerkleHash,
    /// Root hash of the chunk transactions in the given block.
    pub chunk_tx_root: MerkleHash,
    /// Number of chunks included into the block.
    pub chunks_included: u64,
    /// Timestamp at which the block was built.
    pub timestamp: u64,
    /// Approval mask, given current block producers.
    pub approval_mask: Vec<bool>,
    /// Approval signatures for previous block.
    pub approval_sigs: Vec<Signature>,
    /// Total weight.
    pub total_weight: Weight,
    /// Validator proposals.
    pub validator_proposals: Vec<ValidatorStake>,
    /// Mask for new chunks included in the block
    pub chunk_mask: Vec<bool>,
    /// Sum of gas used across all chunks.
    pub gas_used: Gas,
    /// Gas limit. Same for all chunks.
    pub gas_limit: Gas,
    /// Gas price. Same for all chunks
    pub gas_price: Balance,
    /// Sum of all storage rent paid across all chunks.
    pub rent_paid: Balance,
    /// Total supply of tokens in the system
    pub total_supply: Balance,
}

impl BlockHeaderInner {
    pub fn new(
        height: BlockIndex,
        epoch_id: EpochId,
        prev_hash: CryptoHash,
        prev_state_root: MerkleHash,
        tx_root: MerkleHash,
        chunk_receipts_root: MerkleHash,
        chunk_headers_root: MerkleHash,
        chunk_tx_root: MerkleHash,
        chunks_included: u64,
        time: DateTime<Utc>,
        approval_mask: Vec<bool>,
        approval_sigs: Vec<Signature>,
        total_weight: Weight,
        validator_proposals: Vec<ValidatorStake>,
        chunk_mask: Vec<bool>,
        gas_used: Gas,
        gas_limit: Gas,
        gas_price: Balance,
        rent_paid: Balance,
        total_supply: Balance,
    ) -> Self {
        Self {
            height,
            epoch_id,
            prev_hash,
            prev_state_root,
            tx_root,
            chunk_receipts_root,
            chunk_headers_root,
            chunk_tx_root,
            chunks_included,
            timestamp: to_timestamp(time),
            approval_mask,
            approval_sigs,
            total_weight,
            validator_proposals,
            chunk_mask,
            gas_used,
            gas_limit,
            gas_price,
            rent_paid,
            total_supply,
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Eq, PartialEq)]
#[borsh_init(init)]
pub struct BlockHeader {
    /// Inner part of the block header that gets hashed.
    pub inner: BlockHeaderInner,

    /// Signature of the block producer.
    pub signature: Signature,

    /// Cached value of hash for this block.
    #[borsh_skip]
    pub hash: CryptoHash,
}

impl BlockHeader {
    pub fn init(&mut self) {
        self.hash = hash(&self.inner.try_to_vec().expect("Failed to serialize"));
    }

    pub fn new(
        height: BlockIndex,
        prev_hash: CryptoHash,
        prev_state_root: MerkleHash,
        tx_root: MerkleHash,
        chunk_receipts_root: MerkleHash,
        chunk_headers_root: MerkleHash,
        chunk_tx_root: MerkleHash,
        chunks_included: u64,
        timestamp: DateTime<Utc>,
        approval_mask: Vec<bool>,
        approval_sigs: Vec<Signature>,
        total_weight: Weight,
        validator_proposals: Vec<ValidatorStake>,
        chunk_mask: Vec<bool>,
        epoch_id: EpochId,
        gas_used: Gas,
        gas_limit: Gas,
        gas_price: Balance,
        rent_paid: Balance,
        total_supply: Balance,
        signer: Arc<dyn Signer>,
    ) -> Self {
        let inner = BlockHeaderInner::new(
            height,
            epoch_id,
            prev_hash,
            prev_state_root,
            tx_root,
            chunk_receipts_root,
            chunk_headers_root,
            chunk_tx_root,
            chunks_included,
            timestamp,
            approval_mask,
            approval_sigs,
            total_weight,
            validator_proposals,
            chunk_mask,
            gas_used,
            gas_limit,
            gas_price,
            rent_paid,
            total_supply,
        );
        let hash = hash(&inner.try_to_vec().expect("Failed to serialize"));
        Self { inner, signature: signer.sign(hash.as_ref()), hash }
    }

    pub fn genesis(
        state_root: MerkleHash,
        chunk_receipts_root: MerkleHash,
        chunk_headers_root: MerkleHash,
        chunk_tx_root: MerkleHash,
        chunks_included: u64,
        timestamp: DateTime<Utc>,
        initial_gas_limit: Gas,
        initial_gas_price: Balance,
        initial_total_supply: Balance,
    ) -> Self {
        let inner = BlockHeaderInner::new(
            0,
            EpochId::default(),
            CryptoHash::default(),
            state_root,
            MerkleHash::default(),
            chunk_receipts_root,
            chunk_headers_root,
            chunk_tx_root,
            chunks_included,
            timestamp,
            vec![],
            vec![],
            0.into(),
            vec![],
            vec![],
            0,
            initial_gas_limit,
            initial_gas_price,
            0,
            initial_total_supply,
        );
        let hash = hash(&inner.try_to_vec().expect("Failed to serialize"));
        Self { inner, signature: Signature::empty(KeyType::ED25519), hash }
    }

    pub fn hash(&self) -> CryptoHash {
        self.hash
    }

    /// Verifies that given public key produced the block.
    pub fn verify_block_producer(&self, public_key: &PublicKey) -> bool {
        self.signature.verify(self.hash.as_ref(), public_key)
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        from_timestamp(self.inner.timestamp)
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Eq, PartialEq)]
pub struct Block {
    pub header: BlockHeader,
    pub chunks: Vec<ShardChunkHeader>,
    pub transactions: Vec<SignedTransaction>,
}

impl Block {
    /// Returns genesis block for given genesis date and state root.
    pub fn genesis(
        state_roots: Vec<MerkleHash>,
        timestamp: DateTime<Utc>,
        num_shards: ShardId,
        initial_gas_limit: Gas,
        initial_gas_price: Balance,
        initial_total_supply: Balance,
    ) -> Self {
        assert!(state_roots.len() == 1 || state_roots.len() == (num_shards as usize));
        let chunks = (0..num_shards)
            .map(|i| {
                ShardChunkHeader::new(
                    CryptoHash::default(),
                    state_roots[i as usize % state_roots.len()],
                    CryptoHash::default(),
                    0,
                    0,
                    i,
                    0,
                    initial_gas_limit,
                    0,
                    CryptoHash::default(),
                    CryptoHash::default(),
                    vec![],
                    Arc::new(EmptySigner {}),
                )
            })
            .collect();
        Block {
            header: BlockHeader::genesis(
                Block::compute_state_root(&chunks),
                Block::compute_chunk_receipts_root(&chunks),
                Block::compute_chunk_headers_root(&chunks),
                Block::compute_chunk_tx_root(&chunks),
                Block::compute_chunks_included(&chunks, 0),
                timestamp,
                initial_gas_limit,
                initial_gas_price,
                initial_total_supply,
            ),
            chunks,
            transactions: vec![],
        }
    }

    /// Produces new block from header of previous block, current state root and set of transactions.
    pub fn produce(
        prev: &BlockHeader,
        height: BlockIndex,
        chunks: Vec<ShardChunkHeader>,
        epoch_id: EpochId,
        transactions: Vec<SignedTransaction>,
        mut approvals: HashMap<usize, Signature>,
        gas_price_adjustment_rate: u8,
        inflation: Option<Balance>,
        signer: Arc<dyn Signer>,
    ) -> Self {
        let (approval_mask, approval_sigs) = if let Some(max_approver) = approvals.keys().max() {
            (
                (0..=*max_approver).map(|i| approvals.contains_key(&i)).collect(),
                (0..=*max_approver).filter_map(|i| approvals.remove(&i)).collect(),
            )
        } else {
            (vec![], vec![])
        };

        // Collect aggregate of validators and gas usage/limits from chunks.
        let mut validator_proposals = vec![];
        let mut gas_used = 0;
        let mut gas_limit = 0;
        // This computation of chunk_mask relies on the fact that chunks are ordered by shard_id.
        let mut chunk_mask = vec![];
        let mut storage_rent = 0;
        for chunk in chunks.iter() {
            if chunk.height_included == height {
                validator_proposals.extend_from_slice(&chunk.inner.validator_proposals);
                gas_used += chunk.inner.gas_used;
                gas_limit += chunk.inner.gas_limit;
                storage_rent += chunk.inner.rent_paid;
                chunk_mask.push(true);
            } else {
                chunk_mask.push(false);
            }
        }

        let new_gas_price = if gas_limit > 0 {
            (2 * u128::from(gas_limit) + 2 * u128::from(gas_price_adjustment_rate)
                - u128::from(gas_limit) * u128::from(gas_price_adjustment_rate))
                * prev.inner.gas_price
                / (2 * u128::from(gas_limit) * 100)
        } else {
            // If there are no new chunks included in this block, use previous price.
            prev.inner.gas_price
        };
        let new_total_supply = prev.inner.total_supply + inflation.unwrap_or(0);

        let total_weight =
            (prev.inner.total_weight.to_num() + (approval_sigs.len() as u64) + 1).into();
        Block {
            header: BlockHeader::new(
                height,
                prev.hash(),
                Block::compute_state_root(&chunks),
                Block::compute_tx_root(&transactions),
                Block::compute_chunk_receipts_root(&chunks),
                Block::compute_chunk_headers_root(&chunks),
                Block::compute_chunk_tx_root(&chunks),
                Block::compute_chunks_included(&chunks, height),
                Utc::now(),
                approval_mask,
                approval_sigs,
                total_weight,
                validator_proposals,
                chunk_mask,
                epoch_id,
                gas_used,
                gas_limit,
                // TODO: calculate this correctly
                new_gas_price,
                storage_rent,
                new_total_supply,
                signer,
            ),
            chunks,
            transactions,
        }
    }

    pub fn compute_state_root(chunks: &Vec<ShardChunkHeader>) -> CryptoHash {
        merklize(
            &chunks.iter().map(|chunk| chunk.inner.prev_state_root).collect::<Vec<CryptoHash>>(),
        )
        .0
    }

    pub fn compute_tx_root(transactions: &Vec<SignedTransaction>) -> CryptoHash {
        merklize(&transactions.iter().map(|tx| tx.get_hash()).collect::<Vec<CryptoHash>>()).0
    }

    pub fn compute_chunk_receipts_root(chunks: &Vec<ShardChunkHeader>) -> CryptoHash {
        merklize(
            &chunks
                .iter()
                .map(|chunk| chunk.inner.outgoing_receipts_root)
                .collect::<Vec<CryptoHash>>(),
        )
        .0
    }

    pub fn compute_chunk_headers_root(chunks: &Vec<ShardChunkHeader>) -> CryptoHash {
        merklize(
            &chunks
                .iter()
                .map(|chunk| ChunkHashHeight(chunk.hash.clone(), chunk.height_included))
                .collect::<Vec<ChunkHashHeight>>(),
        )
        .0
    }

    pub fn compute_chunk_tx_root(chunks: &Vec<ShardChunkHeader>) -> CryptoHash {
        merklize(&chunks.iter().map(|chunk| chunk.inner.tx_root).collect::<Vec<CryptoHash>>()).0
    }

    pub fn compute_chunks_included(chunks: &Vec<ShardChunkHeader>, height: BlockIndex) -> u64 {
        chunks.iter().filter(|chunk| chunk.height_included == height).count() as u64
    }

    pub fn hash(&self) -> CryptoHash {
        self.header.hash()
    }
}

/// The weight is defined as the number of unique validators approving this fork.
#[derive(
    BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Default,
)]
pub struct Weight {
    num: u64,
}

impl Weight {
    pub fn to_num(self) -> u64 {
        self.num
    }

    pub fn next(self, num: u64) -> Self {
        Weight { num: self.num + num + 1 }
    }
}

impl From<u64> for Weight {
    fn from(num: u64) -> Self {
        Weight { num }
    }
}

impl std::fmt::Display for Weight {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.num)
    }
}
