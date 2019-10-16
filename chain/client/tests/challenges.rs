use std::collections::{HashMap, HashSet};
use std::time::Duration;

use borsh::BorshSerialize;

use near::GenesisConfig;
use near_chain::types::validate_challenge;
use near_chain::{Block, ChainGenesis, ChainStoreAccess, Provenance, RuntimeAdapter};
use near_client::test_utils::TestEnv;
use near_client::Client;
use near_crypto::InMemoryBlsSigner;
use near_network::types::{ChunkOnePartRequestMsg, PeerId};
use near_network::NetworkRequests;
use near_primitives::challenge::{BlockDoubleSign, Challenge, ChallengeBody, ChunkProofs};
use near_primitives::hash::{hash, CryptoHash};
use near_primitives::merkle::MerklePath;
use near_primitives::receipt::Receipt;
use near_primitives::serialize::BaseDecode;
use near_primitives::sharding::{ChunkHash, EncodedShardChunk};
use near_primitives::test_utils::init_test_logger;
use near_store::test_utils::create_test_store;
use std::path::Path;
use std::sync::Arc;

#[test]
fn test_verify_block_double_sign_challenge() {
    let mut env = TestEnv::new(ChainGenesis::test(), 2, 1);
    env.produce_block(0, 1);
    let genesis = env.clients[0].chain.get_block_by_height(0).unwrap().clone();
    let b1 = env.clients[0].produce_block(2, Duration::from_millis(10)).unwrap().unwrap();
    let signer = InMemoryBlsSigner::from_seed("test0", "test0");
    let b2 = Block::produce(
        &genesis.header,
        2,
        genesis.chunks.clone(),
        b1.header.inner.epoch_id.clone(),
        HashMap::default(),
        0,
        None,
        vec![],
        &signer,
    );
    let epoch_id = b1.header.inner.epoch_id.clone();
    let valid_challenge = Challenge::produce(
        ChallengeBody::BlockDoubleSign(BlockDoubleSign {
            left_block_header: b1.header.try_to_vec().unwrap(),
            right_block_header: b2.header.try_to_vec().unwrap(),
        }),
        signer.account_id.clone(),
        &signer,
    );
    assert_eq!(
        validate_challenge(&*env.clients[1].chain.runtime_adapter, &epoch_id, &valid_challenge)
            .unwrap()
            .0,
        if b1.hash() > b2.hash() { b1.hash() } else { b2.hash() }
    );
    let invalid_challenge = Challenge::produce(
        ChallengeBody::BlockDoubleSign(BlockDoubleSign {
            left_block_header: b1.header.try_to_vec().unwrap(),
            right_block_header: b1.header.try_to_vec().unwrap(),
        }),
        signer.account_id.clone(),
        &signer,
    );
    assert!(validate_challenge(
        &*env.clients[1].chain.runtime_adapter,
        &epoch_id,
        &invalid_challenge
    )
    .is_err());
    let b3 = env.clients[0].produce_block(3, Duration::from_millis(10)).unwrap().unwrap();
    let invalid_challenge = Challenge::produce(
        ChallengeBody::BlockDoubleSign(BlockDoubleSign {
            left_block_header: b1.header.try_to_vec().unwrap(),
            right_block_header: b3.header.try_to_vec().unwrap(),
        }),
        signer.account_id.clone(),
        &signer,
    );
    assert!(validate_challenge(
        &*env.clients[1].chain.runtime_adapter,
        &epoch_id,
        &invalid_challenge
    )
    .is_err());
}

fn create_invalid_proofs_chunk(
    client: &mut Client,
) -> (EncodedShardChunk, Vec<MerklePath>, Vec<Receipt>, Block) {
    let last_block = client.chain.get_block_by_height(1).unwrap().clone();
    let (mut chunk, merkle_paths, receipts) = client
        .produce_chunk(
            last_block.hash(),
            &last_block.header.inner.epoch_id,
            last_block.chunks[0].clone(),
            2,
            0,
            0,
        )
        .unwrap()
        .unwrap();
    chunk.header.inner.tx_root =
        CryptoHash::from_base("F5SvmQcKqekuKPJgLUNFgjB4ZgVmmiHsbDhTBSQbiywf").unwrap();
    chunk.header.height_included = 2;
    chunk.header.hash = ChunkHash(hash(&chunk.header.inner.try_to_vec().unwrap()));
    chunk.header.signature =
        client.block_producer.as_ref().unwrap().signer.sign(chunk.header.hash.as_ref());
    let block = Block::produce(
        &last_block.header,
        2,
        vec![chunk.header.clone()],
        last_block.header.inner.epoch_id.clone(),
        HashMap::default(),
        0,
        None,
        vec![],
        &*client.block_producer.as_ref().unwrap().signer,
    );
    (chunk, merkle_paths, receipts, block)
}

#[test]
fn test_verify_chunk_invalid_proofs_challenge() {
    let mut env = TestEnv::new(ChainGenesis::test(), 1, 1);
    env.produce_block(0, 1);
    let (chunk, _merkle_paths, _receipts, block) = create_invalid_proofs_chunk(&mut env.clients[0]);

    let merkle_paths = Block::compute_chunk_headers_root(&block.chunks).1;
    let valid_challenge = Challenge::produce(
        ChallengeBody::ChunkProofs(ChunkProofs {
            block_header: block.header.try_to_vec().unwrap(),
            chunk: chunk.clone(),
            merkle_proof: merkle_paths[chunk.header.inner.shard_id as usize].clone(),
        }),
        env.clients[0].block_producer.as_ref().unwrap().account_id.clone(),
        &*env.clients[0].block_producer.as_ref().unwrap().signer,
    );
    assert_eq!(
        validate_challenge(
            &*env.clients[0].chain.runtime_adapter,
            &block.header.inner.epoch_id,
            &valid_challenge
        )
        .unwrap(),
        (block.hash(), vec!["test0".to_string()])
    );
}

#[test]
fn test_verify_chunk_invalid_state_challenge() {
    let store1 = create_test_store();
    let genesis_config = GenesisConfig::test(vec!["test0"], 1);
    let runtimes: Vec<Arc<dyn RuntimeAdapter>> = vec![Arc::new(near::NightshadeRuntime::new(
        Path::new("."),
        store1,
        genesis_config,
        vec![],
        vec![],
    ))];
    let mut env = TestEnv::new_with_runtime(ChainGenesis::test(), 1, 1, runtimes);
}

#[test]
fn test_request_chunk_restart() {
    init_test_logger();
    let mut env = TestEnv::new(ChainGenesis::test(), 1, 1);
    for i in 1..4 {
        env.produce_block(0, i);
        env.network_adapters[0].pop();
    }
    let block1 = env.clients[0].chain.get_block_by_height(3).unwrap().clone();
    let request = ChunkOnePartRequestMsg {
        shard_id: 0,
        chunk_hash: block1.chunks[0].chunk_hash(),
        height: block1.header.inner.height,
        part_id: 0,
        tracking_shards: HashSet::default(),
    };
    let client = &mut env.clients[0];
    client
        .shards_mgr
        .process_chunk_one_part_request(request.clone(), PeerId::random(), client.chain.mut_store())
        .unwrap();
    assert!(env.network_adapters[0].pop().is_some());

    env.restart(0);
    let client = &mut env.clients[0];
    client
        .shards_mgr
        .process_chunk_one_part_request(request, PeerId::random(), client.chain.mut_store())
        .unwrap();
    // TODO: should be some() with the same chunk.
    assert!(env.network_adapters[0].pop().is_none());
}

/// Receive invalid state transition in chunk as next chunk producer.
#[test]
fn test_receive_invalid_chunk_as_chunk_producer() {
    init_test_logger();
    let mut env = TestEnv::new(ChainGenesis::test(), 1, 1);
    env.produce_block(0, 1);
    env.network_adapters[0].pop().unwrap();
    let (chunk, merkle_paths, receipts, block) = create_invalid_proofs_chunk(&mut env.clients[0]);
    let client = &mut env.clients[0];
    assert!(client
        .shards_mgr
        .distribute_encoded_chunk(chunk.clone(), merkle_paths, receipts, client.chain.mut_store(),)
        .is_err());
    let (_, result) = client.process_block(block.clone(), Provenance::NONE);
    // We have declined block with invalid chunk, but everyone who doesn't track this shard have accepted.
    assert!(result.is_err());
    assert_eq!(client.chain.head().unwrap().height, 1);
    // At this point we should create a challenge and send it out.
    let last_message = env.network_adapters[0].pop().unwrap();
    if let NetworkRequests::Challenge(Challenge {
        body: ChallengeBody::ChunkProofs(chunk_proofs),
        ..
    }) = last_message
    {
        assert_eq!(chunk_proofs.chunk, chunk);
    } else {
        assert!(false);
    }
}

/// Receive invalid state transition in chunk as a validator / non-producer.
#[test]
fn test_receive_invalid_chunk_as_validator() {}

/// Receive two different chunks from the same chunk producer.
#[test]
fn test_receive_two_chunks_from_one_producer() {}

/// Receive two different blocks from the same block producer.
#[test]
fn test_receive_two_blocks_from_one_producer() {}

/// Receive challenges in the blocks.
#[test]
fn test_block_challenge() {
    init_test_logger();
    let mut env = TestEnv::new(ChainGenesis::test(), 1, 1);
    env.produce_block(0, 1);
    let (chunk, _merkle_paths, _receipts, block) = create_invalid_proofs_chunk(&mut env.clients[0]);

    let merkle_paths = Block::compute_chunk_headers_root(&block.chunks).1;
    let challenge = Challenge::produce(
        ChallengeBody::ChunkProofs(ChunkProofs {
            block_header: block.header.try_to_vec().unwrap(),
            chunk: chunk.clone(),
            merkle_proof: merkle_paths[chunk.header.inner.shard_id as usize].clone(),
        }),
        env.clients[0].block_producer.as_ref().unwrap().account_id.clone(),
        &*env.clients[0].block_producer.as_ref().unwrap().signer,
    );
    env.clients[0].process_challenge(challenge.clone()).unwrap();
    env.produce_block(0, 2);
    assert_eq!(
        env.clients[0].chain.get_block_by_height(2).unwrap().header.inner.challenges,
        vec![challenge]
    );
    assert!(env.clients[0].chain.mut_store().is_block_challenged(&block.hash()).unwrap());
}
