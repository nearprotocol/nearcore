use std::collections::HashMap;

use borsh::{BorshDeserialize, BorshSerialize};

use near_crypto::{BlsSignature, BlsSigner};
pub use near_primitives::block::{Block, BlockHeader, Weight};
use near_primitives::challenge::{
    BlockDoubleSign, Challenge, ChallengeBody, ChallengesResult, ChunkProofs, ChunkState,
};
use near_primitives::errors::InvalidTxErrorOrStorageError;
use near_primitives::hash::{hash, CryptoHash};
use near_primitives::merkle::{merklize, MerklePath};
use near_primitives::receipt::Receipt;
use near_primitives::sharding::{
    ChunkHash, EncodedShardChunk, ReceiptProof, ShardChunk, ShardChunkHeader,
};
use near_primitives::transaction::{ExecutionOutcomeWithId, SignedTransaction};
use near_primitives::types::{
    AccountId, Balance, BlockIndex, EpochId, Gas, ShardId, StateRoot, ValidatorStake,
};
use near_primitives::views::QueryResponse;
use near_store::{PartialStorage, StoreUpdate, Trie, WrappedTrieChanges};

use crate::error::Error;
use crate::{byzantine_assert, ErrorKind};

#[derive(PartialEq, Eq, Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct ReceiptResponse(pub CryptoHash, pub Vec<Receipt>);

#[derive(PartialEq, Eq, Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct ReceiptProofResponse(pub CryptoHash, pub Vec<ReceiptProof>);

#[derive(PartialEq, Eq, Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct RootProof(pub CryptoHash, pub MerklePath);

#[derive(PartialEq, Eq, Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct StateHeaderKey(pub ShardId, pub CryptoHash);

#[derive(PartialEq, Eq, Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct StatePartKey(pub u64, pub StateRoot);

#[derive(PartialEq, Eq, Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct StatePart {
    pub shard_id: ShardId,
    pub part_id: u64,
    pub data: Vec<u8>,
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub enum BlockStatus {
    /// Block is the "next" block, updating the chain head.
    Next,
    /// Block does not update the chain head and is a fork.
    Fork,
    /// Block updates the chain head via a (potentially disruptive) "reorg".
    /// Previous block was not our previous chain head.
    Reorg(CryptoHash),
}

impl BlockStatus {
    pub fn is_new_head(&self) -> bool {
        match self {
            BlockStatus::Next => true,
            BlockStatus::Fork => false,
            BlockStatus::Reorg(_) => true,
        }
    }
}

/// Options for block origin.
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum Provenance {
    /// No provenance.
    NONE,
    /// Adds block while in syncing mode.
    SYNC,
    /// Block we produced ourselves.
    PRODUCED,
}

/// Information about processed block.
#[derive(Debug, Clone)]
pub struct AcceptedBlock {
    pub hash: CryptoHash,
    pub status: BlockStatus,
    pub provenance: Provenance,
    pub gas_used: Gas,
    pub gas_limit: Gas,
}

/// Information about valid transaction that was processed by chain + runtime.
#[derive(Debug)]
pub struct ValidTransaction {
    pub transaction: SignedTransaction,
}

/// Map of shard to list of receipts to send to it.
pub type ReceiptResult = HashMap<ShardId, Vec<Receipt>>;

#[derive(Eq, PartialEq, Debug)]
pub enum ValidatorSignatureVerificationResult {
    Valid,
    Invalid,
    UnknownEpoch,
}

impl ValidatorSignatureVerificationResult {
    pub fn valid(&self) -> bool {
        *self == ValidatorSignatureVerificationResult::Valid
    }
}

pub struct ApplyTransactionResult {
    pub trie_changes: WrappedTrieChanges,
    pub new_root: StateRoot,
    pub transaction_results: Vec<ExecutionOutcomeWithId>,
    pub receipt_result: ReceiptResult,
    pub validator_proposals: Vec<ValidatorStake>,
    pub total_gas_burnt: Gas,
    pub total_rent_paid: Balance,
    pub proof: Option<PartialStorage>,
}

/// Bridge between the chain and the runtime.
/// Main function is to update state given transactions.
/// Additionally handles validators and block weight computation.
pub trait RuntimeAdapter: Send + Sync {
    /// Initialize state to genesis state and returns StoreUpdate, state root and initial validators.
    /// StoreUpdate can be discarded if the chain past the genesis.
    fn genesis_state(&self) -> (StoreUpdate, Vec<StateRoot>);

    /// Verify block producer validity and return weight of given block for fork choice rule.
    fn compute_block_weight(
        &self,
        prev_header: &BlockHeader,
        header: &BlockHeader,
    ) -> Result<Weight, Error>;

    /// Validate transaction and return transaction information relevant to ordering it in the mempool.
    fn validate_tx(
        &self,
        block_index: BlockIndex,
        block_timestamp: u64,
        gas_price: Balance,
        state_root: StateRoot,
        transaction: SignedTransaction,
    ) -> Result<ValidTransaction, InvalidTxErrorOrStorageError>;

    /// Filter transactions by verifying each one by one in the given order. Every successful
    /// verification stores the updated account balances to be used by next transactions.
    fn filter_transactions(
        &self,
        block_index: BlockIndex,
        block_timestamp: u64,
        gas_price: Balance,
        state_root: StateRoot,
        transactions: Vec<SignedTransaction>,
    ) -> Vec<SignedTransaction>;

    /// Verify validator signature for the given epoch.
    fn verify_validator_signature(
        &self,
        epoch_id: &EpochId,
        account_id: &AccountId,
        data: &[u8],
        signature: &BlsSignature,
    ) -> ValidatorSignatureVerificationResult;

    /// Verify chunk header signature.
    fn verify_chunk_header_signature(&self, header: &ShardChunkHeader) -> Result<bool, Error>;

    /// Verify aggregated bls signature
    fn verify_approval_signature(
        &self,
        epoch_id: &EpochId,
        last_known_block_hash: &CryptoHash,
        approval_mask: &[bool],
        approval_sig: &BlsSignature,
        data: &[u8],
    ) -> Result<bool, Error>;

    /// Epoch block producers (ordered by their order in the proposals) for given shard.
    /// Returns error if height is outside of known boundaries.
    fn get_epoch_block_producers(
        &self,
        epoch_id: &EpochId,
        last_known_block_hash: &CryptoHash,
    ) -> Result<Vec<(AccountId, bool)>, Error>;

    /// Block producers for given height for the main block. Return error if outside of known boundaries.
    fn get_block_producer(
        &self,
        epoch_id: &EpochId,
        height: BlockIndex,
    ) -> Result<AccountId, Error>;

    /// Chunk producer for given height for given shard. Return error if outside of known boundaries.
    fn get_chunk_producer(
        &self,
        epoch_id: &EpochId,
        height: BlockIndex,
        shard_id: ShardId,
    ) -> Result<AccountId, Error>;

    /// Get current number of shards.
    fn num_shards(&self) -> ShardId;

    fn num_total_parts(&self, parent_hash: &CryptoHash) -> usize;

    fn num_data_parts(&self, parent_hash: &CryptoHash) -> usize;

    /// Account Id to Shard Id mapping, given current number of shards.
    fn account_id_to_shard_id(&self, account_id: &AccountId) -> ShardId;

    /// Returns `account_id` that suppose to have the `part_id` of all chunks given previous block hash.
    fn get_part_owner(&self, parent_hash: &CryptoHash, part_id: u64) -> Result<AccountId, Error>;

    /// Whether the client cares about some shard right now.
    /// * If `account_id` is None, `is_me` is not checked and the
    /// result indicates whether the client is tracking the shard
    /// * If `account_id` is not None, it is supposed to be a validator
    /// account and `is_me` indicates whether we check what shards
    /// the client tracks.
    fn cares_about_shard(
        &self,
        account_id: Option<&AccountId>,
        parent_hash: &CryptoHash,
        shard_id: ShardId,
        is_me: bool,
    ) -> bool;

    /// Whether the client cares about some shard in the next epoch.
    /// * If `account_id` is None, `is_me` is not checked and the
    /// result indicates whether the client will track the shard
    /// * If `account_id` is not None, it is supposed to be a validator
    /// account and `is_me` indicates whether we check what shards
    /// the client will track.
    fn will_care_about_shard(
        &self,
        account_id: Option<&AccountId>,
        parent_hash: &CryptoHash,
        shard_id: ShardId,
        is_me: bool,
    ) -> bool;

    /// Returns true, if given hash is last block in it's epoch.
    fn is_next_block_epoch_start(&self, parent_hash: &CryptoHash) -> Result<bool, Error>;

    /// Get epoch id given hash of previous block.
    fn get_epoch_id_from_prev_block(&self, parent_hash: &CryptoHash) -> Result<EpochId, Error>;

    /// Get next epoch id given hash of previous block.
    fn get_next_epoch_id_from_prev_block(&self, parent_hash: &CryptoHash)
        -> Result<EpochId, Error>;

    /// Get epoch start for given block hash.
    fn get_epoch_start_height(&self, block_hash: &CryptoHash) -> Result<BlockIndex, Error>;

    /// Get inflation for a certain epoch
    fn get_epoch_inflation(&self, epoch_id: &EpochId) -> Result<Balance, Error>;

    /// Add proposals for validators.
    fn add_validator_proposals(
        &self,
        parent_hash: CryptoHash,
        current_hash: CryptoHash,
        block_index: BlockIndex,
        proposals: Vec<ValidatorStake>,
        slashed_validators: Vec<AccountId>,
        validator_mask: Vec<bool>,
        gas_used: Gas,
        gas_price: Balance,
        rent_paid: Balance,
        total_supply: Balance,
    ) -> Result<(), Error>;

    /// Apply transactions to given state root and return store update and new state root.
    /// Also returns transaction result for each transaction and new receipts.
    fn apply_transactions(
        &self,
        shard_id: ShardId,
        state_root: &StateRoot,
        block_index: BlockIndex,
        block_timestamp: u64,
        prev_block_hash: &CryptoHash,
        block_hash: &CryptoHash,
        receipts: &[Receipt],
        transactions: &[SignedTransaction],
        last_validator_proposals: &[ValidatorStake],
        gas_price: Balance,
        challenges: &ChallengesResult,
    ) -> Result<ApplyTransactionResult, Error> {
        self.apply_transactions_with_optional_storage_proof(
            shard_id,
            state_root,
            block_index,
            block_timestamp,
            prev_block_hash,
            block_hash,
            receipts,
            transactions,
            last_validator_proposals,
            gas_price,
            challenges,
            false,
        )
    }

    fn apply_transactions_with_optional_storage_proof(
        &self,
        shard_id: ShardId,
        state_root: &StateRoot,
        block_index: BlockIndex,
        block_timestamp: u64,
        prev_block_hash: &CryptoHash,
        block_hash: &CryptoHash,
        receipts: &[Receipt],
        transactions: &[SignedTransaction],
        last_validator_proposals: &[ValidatorStake],
        gas_price: Balance,
        challenges: &ChallengesResult,
        generate_storage_proof: bool,
    ) -> Result<ApplyTransactionResult, Error>;

    /// Query runtime with given `path` and `data`.
    fn query(
        &self,
        state_root: &StateRoot,
        height: BlockIndex,
        block_timestamp: u64,
        block_hash: &CryptoHash,
        path_parts: Vec<&str>,
        data: &[u8],
    ) -> Result<QueryResponse, Box<dyn std::error::Error>>;

    /// Get the part of the state from given state root + proof.
    fn obtain_state_part(
        &self,
        shard_id: ShardId,
        part_id: u64,
        state_root: &StateRoot,
    ) -> Result<(StatePart, Vec<u8>), Box<dyn std::error::Error>>;

    /// Set state part that expected to be given state root with provided data.
    /// Returns error if:
    /// 1. Failed to parse, or
    /// 2. The proof is invalid, or
    /// 3. The resulting part doesn't match the expected one.
    fn accept_state_part(
        &self,
        state_root: &StateRoot,
        part: &StatePart,
        proof: &Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Should be executed after accepting all the parts.
    /// Returns `true` if state is set successfully.
    fn confirm_state(&self, state_root: &StateRoot) -> Result<bool, Error>;

    /// Build receipts hashes.
    fn build_receipts_hashes(&self, receipts: &Vec<Receipt>) -> Result<Vec<CryptoHash>, Error> {
        let mut receipts_hashes = vec![];
        for shard_id in 0..self.num_shards() {
            // importance to save the same order while filtering
            let shard_receipts: Vec<Receipt> = receipts
                .iter()
                .filter(|&receipt| self.account_id_to_shard_id(&receipt.receiver_id) == shard_id)
                .cloned()
                .collect();
            receipts_hashes.push(hash(&ReceiptList(shard_id, shard_receipts).try_to_vec()?));
        }
        Ok(receipts_hashes)
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Default)]
pub struct ReceiptList(pub ShardId, pub Vec<Receipt>);

/// The last known / checked height and time when we have processed it.
/// Required to keep track of skipped blocks and not fallback to produce blocks at lower height.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Default)]
pub struct LatestKnown {
    pub height: BlockIndex,
    pub seen: u64,
}

/// The tip of a fork. A handle to the fork ancestry from its leaf in the
/// blockchain tree. References the max height and the latest and previous
/// blocks for convenience and the total weight.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct Tip {
    /// Height of the tip (max height of the fork)
    pub height: BlockIndex,
    /// Last block pushed to the fork
    pub last_block_hash: CryptoHash,
    /// Previous block
    pub prev_block_hash: CryptoHash,
    /// Total weight on that fork
    pub total_weight: Weight,
    /// Previous epoch id. Used for getting validator info.
    pub epoch_id: EpochId,
}

impl Tip {
    /// Creates a new tip based on provided header.
    pub fn from_header(header: &BlockHeader) -> Tip {
        Tip {
            height: header.inner.height,
            last_block_hash: header.hash(),
            prev_block_hash: header.inner.prev_hash,
            total_weight: header.inner.total_weight,
            epoch_id: header.inner.epoch_id.clone(),
        }
    }
}

/// Block approval by other block producers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockApproval {
    pub hash: CryptoHash,
    pub signature: BlsSignature,
    pub target: AccountId,
}

impl BlockApproval {
    pub fn new(hash: CryptoHash, signer: &dyn BlsSigner, target: AccountId) -> Self {
        let signature = signer.sign(hash.as_ref());
        BlockApproval { hash, signature, target }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ShardStateSyncResponseHeader {
    pub chunk: ShardChunk,
    pub chunk_proof: MerklePath,
    pub prev_chunk_header: ShardChunkHeader,
    pub prev_chunk_proof: MerklePath,
    pub incoming_receipts_proofs: Vec<ReceiptProofResponse>,
    pub root_proofs: Vec<Vec<RootProof>>,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ShardStateSyncResponsePart {
    pub state_part: StatePart,
    pub proof: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ShardStateSyncResponse {
    pub header: Option<ShardStateSyncResponseHeader>,
    pub parts: Vec<ShardStateSyncResponsePart>,
}

/// Verifies that chunk's proofs in the header match the body.
pub fn validate_chunk_proofs(
    chunk: &ShardChunk,
    runtime_adapter: &dyn RuntimeAdapter,
) -> Result<bool, Error> {
    // 1. Checking chunk.header.hash
    if chunk.header.hash != ChunkHash(hash(&chunk.header.inner.try_to_vec()?)) {
        byzantine_assert!(false);
        return Ok(false);
    }

    // 2. Checking that chunk body is valid
    // 2a. Checking chunk hash
    if chunk.chunk_hash != chunk.header.hash {
        byzantine_assert!(false);
        return Ok(false);
    }
    // 2b. Checking that chunk transactions are valid
    let (tx_root, _) = merklize(&chunk.transactions);
    if tx_root != chunk.header.inner.tx_root {
        byzantine_assert!(false);
        return Ok(false);
    }
    // 2c. Checking that chunk receipts are valid
    let outgoing_receipts_hashes = runtime_adapter.build_receipts_hashes(&chunk.receipts)?;
    let (receipts_root, _) = merklize(&outgoing_receipts_hashes);
    if receipts_root != chunk.header.inner.outgoing_receipts_root {
        byzantine_assert!(false);
        return Ok(false);
    }
    Ok(true)
}

fn verify_double_sign(
    runtime_adapter: &RuntimeAdapter,
    block_double_sign: &BlockDoubleSign,
) -> Result<(CryptoHash, Vec<AccountId>), Error> {
    let left_block_header = BlockHeader::try_from_slice(&block_double_sign.left_block_header)?;
    let right_block_header = BlockHeader::try_from_slice(&block_double_sign.right_block_header)?;
    let block_producer = runtime_adapter
        .get_block_producer(&left_block_header.inner.epoch_id, left_block_header.inner.height)?;
    if left_block_header.hash() != right_block_header.hash()
        && left_block_header.inner.height == right_block_header.inner.height
        && runtime_adapter
            .verify_validator_signature(
                &left_block_header.inner.epoch_id,
                &block_producer,
                left_block_header.hash().as_ref(),
                &left_block_header.signature,
            )
            .valid()
        && runtime_adapter
            .verify_validator_signature(
                &right_block_header.inner.epoch_id,
                &block_producer,
                right_block_header.hash().as_ref(),
                &right_block_header.signature,
            )
            .valid()
    {
        // Deterministically return header with higher hash.
        Ok(if left_block_header.hash() > right_block_header.hash() {
            (left_block_header.hash(), vec![block_producer])
        } else {
            (right_block_header.hash(), vec![block_producer])
        })
    } else {
        Err(ErrorKind::MaliciousChallenge.into())
    }
}

fn verify_header_authorship(block_header: &BlockHeader) -> Result<(), Error> {
    let block_producer = runtime_adapter
        .get_block_producer(&block_header.inner.epoch_id, block_header.inner.height)?;
    match runtime_adapter.verify_validator_signature(
        &block_header.inner.epoch_id,
        &block_producer,
        block_header.hash().as_ref(),
        &block_header.signature,
    ) {
        ValidatorSignatureVerificationResult::Valid => {}
        ValidatorSignatureVerificationResult::Invalid => {
            return Err(ErrorKind::InvalidChallenge.into())
        }
        ValidatorSignatureVerificationResult::UnknownEpoch => {
            return Err(ErrorKind::EpochOutOfBounds.into())
        }
    }
    Ok(())
}

fn verify_chunk_authorship(
    block_header: &BlockHeader,
    chunk_header: &ShardChunkHeader,
) -> Result<(), Error> {
    let chunk_producer = runtime_adapter.get_chunk_producer(
        &block_header.inner.epoch_id,
        chunk_header.inner.height_created,
        chunk_header.inner.shard_id,
    )?;
    match runtime_adapter.verify_validator_signature(
        &block_header.inner.epoch_id,
        &chunk_producer,
        chunk_header.chunk_hash().as_ref(),
        &chunk_header.signature,
    ) {
        ValidatorSignatureVerificationResult::Valid => {}
        ValidatorSignatureVerificationResult::Invalid => {
            return Err(ErrorKind::InvalidChallenge.into())
        }
        ValidatorSignatureVerificationResult::UnknownEpoch => {
            return Err(ErrorKind::EpochOutOfBounds.into())
        }
    };
    Ok(())
}

fn verify_chunk_proofs_challenge(
    runtime_adapter: &RuntimeAdapter,
    chunk_proofs: &ChunkProofs,
) -> Result<(CryptoHash, Vec<AccountId>), Error> {
    let block_header = BlockHeader::try_from_slice(&chunk_proofs.block_header)?;
    verify_header_authorship(&block_header)?;
    verify_chunk_authorship(&block_header, &chunk_proofs.chunk.header)?;
    if !Block::validate_chunk_header_proof(
        &chunk_proofs.chunk.header,
        &block_header.inner.chunk_headers_root,
        &chunk_proofs.merkle_proof,
    ) {
        return Err(ErrorKind::MaliciousChallenge.into());
    }
    match chunk_proofs
        .chunk
        .decode_chunk(
            runtime_adapter.num_data_parts(&chunk_proofs.chunk.header.inner.prev_block_hash),
        )
        .map_err(|err| err.into())
        .and_then(|chunk| validate_chunk_proofs(&chunk, &*runtime_adapter))
    {
        Ok(true) => Err(ErrorKind::MaliciousChallenge.into()),
        Ok(false) | Err(_) => Ok((block_header.hash(), vec![chunk_producer])),
    }
}

fn verify_chunk_state_challenge(
    runtime_adapter: &RuntimeAdapter,
    chunk_state: &ChunkState,
) -> Result<(CryptoHash, Vec<AccountId>), Error> {
    let block_header = BlockHeader::try_from_slice(&chunk_state.block_header)?;

    verify_header_authorship(&block_header)?;
    verify_chunk_authorship(&block_header, &chunk_state.prev_chunk.header)?;
    verify_chunk_authorship(&block_header, &chunk_state.chunk_header)?;

    // TODO: verify inclusion of prev_chunk into this chain.
    runtime_adapter.apply_transactions(
        chunk_state.chunk_header.inner.shard_id,
        &chunk_state.chunk_header.inner.prev_state_root,
        block_header.inner.height,
        block_header.inner.timestamp,
        block_header.inner.prev_block_hash,
        block_header.hash(),
        &[],
        &[],
        &[],
        0,
        &[],
    )?;
    // runtime_adapter.check_transactions();
    // let trie =
    // Retrieve block, if it's missing return error to fetch it.
    //                let prev_chunk_header =
    //                    self.store.get_block(&block_hash)?.chunks[shard_id as usize].clone();
    //                let prev_chunk = self.store.get_chunk_clone_from_header(&prev_chunk_header)?;
    // chunk_header.inner.
    // TODO: TODO
    Err(ErrorKind::MaliciousChallenge.into())
}

/// Returns Some(block hash, vec![account_id]) of invalid block and who to slash if challenge is correct and None if incorrect.
pub fn verify_challenge(
    runtime_adapter: &RuntimeAdapter,
    epoch_id: &EpochId,
    challenge: &Challenge,
) -> Result<(CryptoHash, Vec<AccountId>), Error> {
    // Check signature is correct on the challenge.
    if !runtime_adapter
        .verify_validator_signature(
            epoch_id,
            &challenge.account_id,
            challenge.hash.as_ref(),
            &challenge.signature,
        )
        .valid()
    {
        return Err(ErrorKind::InvalidChallenge.into());
    }
    match &challenge.body {
        ChallengeBody::BlockDoubleSign(block_double_sign) => {
            verify_double_sign(runtime_adapter, block_double_sign)
        }
        ChallengeBody::ChunkProofs(chunk_proofs) => {
            verify_chunk_proofs_challenge(runtime_adapter, chunk_proofs)
        }
        ChallengeBody::ChunkState(chunk_state) => {
            verify_chunk_state_challenge(runtime_adapter, chunk_state)
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use near_crypto::{BlsSignature, InMemoryBlsSigner};

    use super::*;

    #[test]
    fn test_block_produce() {
        let num_shards = 32;
        let genesis = Block::genesis(
            vec![StateRoot { hash: CryptoHash::default(), num_parts: 9 /* TODO MOO */ }],
            Utc::now(),
            num_shards,
            1_000_000,
            100,
            1_000_000_000,
        );
        let signer = InMemoryBlsSigner::from_seed("other", "other");
        let b1 = Block::empty(&genesis, &signer);
        assert!(signer.verify(b1.hash().as_ref(), &b1.header.signature));
        assert_eq!(b1.header.inner.total_weight.to_num(), 1);
        let other_signer = InMemoryBlsSigner::from_seed("other2", "other2");
        let approvals: HashMap<usize, BlsSignature> =
            vec![(1, other_signer.sign(b1.hash().as_ref()))].into_iter().collect();
        let b2 = Block::empty_with_approvals(
            &b1,
            2,
            b1.header.inner.epoch_id.clone(),
            approvals,
            &signer,
        );
        assert!(signer.verify(b2.hash().as_ref(), &b2.header.signature));
        assert_eq!(b2.header.inner.total_weight.to_num(), 3);
    }
}
