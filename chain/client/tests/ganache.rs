use std::collections::{HashSet, VecDeque};
use std::iter::FromIterator;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use actix::System;
use futures::{future, FutureExt};
use num_rational::Rational;

use near_actix_test_utils::run_actix_until_stop;
use near_chain::chain::NUM_EPOCHS_TO_KEEP_STORE_DATA;
use near_chain::types::LatestKnown;
use near_chain::validate::validate_chunk_with_chunk_extra;
use near_chain::{
    Block, ChainGenesis, ChainStore, ChainStoreAccess, ErrorKind, Provenance, RuntimeAdapter,
};
use near_chain_configs::{ClientConfig, Genesis};
use near_chunks::{ChunkStatus, ShardsManager};
use near_client::test_utils::{create_chunk_on_height, setup_mock_all_validators};
use near_client::test_utils::{setup_client, setup_mock, TestEnv};
use near_client::{Client, GetBlock, GetBlockWithMerkleTree};
use near_crypto::{InMemorySigner, KeyType, PublicKey, Signature, Signer};
use near_logger_utils::init_test_logger;
#[cfg(feature = "metric_recorder")]
use near_network::recorder::MetricRecorder;
use near_network::routing::EdgeInfo;
use near_network::test_utils::{wait_or_panic, MockNetworkAdapter};
use near_network::types::{NetworkInfo, PeerChainInfoV2, ReasonForBan};
use near_network::{
    FullPeerInfo, NetworkClientMessages, NetworkClientResponses, NetworkRequests, NetworkResponses,
    PeerInfo,
};
use near_primitives::block::{Approval, ApprovalInner};
use near_primitives::block_header::BlockHeader;
use near_primitives::errors::InvalidTxError;
use near_primitives::hash::{hash, CryptoHash};
use near_primitives::merkle::verify_hash;
#[cfg(not(feature = "protocol_feature_block_header_v3"))]
use near_primitives::sharding::ShardChunkHeaderV2;
use near_primitives::sharding::{EncodedShardChunk, ReedSolomonWrapper, ShardChunkHeader};
#[cfg(feature = "protocol_feature_block_header_v3")]
use near_primitives::sharding::{ShardChunkHeaderInner, ShardChunkHeaderV3};

use near_primitives::receipt::DelayedReceiptIndices;
use near_primitives::serialize::from_base64;
use near_primitives::state_record::StateRecord;
use near_primitives::syncing::{get_num_state_parts, ShardStateSyncResponseHeader};
use near_primitives::transaction::{
    Action, DeployContractAction, ExecutionStatus, FunctionCallAction, SignedTransaction,
    Transaction,
};
use near_primitives::trie_key::TrieKey;
use near_primitives::types::validator_stake::ValidatorStake;
use near_primitives::types::{AccountId, BlockHeight, EpochId, NumBlocks, StoreKey};
use near_primitives::utils::to_timestamp;
use near_primitives::validator_signer::{InMemoryValidatorSigner, ValidatorSigner};
use near_primitives::version::PROTOCOL_VERSION;
use near_primitives::views::{
    BlockHeaderView, FinalExecutionStatus, QueryRequest, QueryResponseKind,
};
use near_store::get;
use near_store::test_utils::create_test_store;
use neard::config::{GenesisExt, TESTING_INIT_BALANCE, TESTING_INIT_STAKE};
use neard::NEAR_BASE;

#[cfg(feature = "ganache")]
#[test]
fn test_patch_state() {
    let epoch_length = 5;
    let mut genesis = Genesis::test(vec!["test0", "test1"], 1);
    genesis.config.epoch_length = epoch_length;
    let mut env = TestEnv::new_with_runtime(
        ChainGenesis::test(),
        1,
        1,
        vec![Arc::new(neard::NightshadeRuntime::new(
            Path::new("."),
            create_test_store(),
            &genesis,
            vec![],
            vec![],
            None,
        )) as Arc<dyn RuntimeAdapter>],
    );
    let genesis_block = env.clients[0].chain.get_block_by_height(0).unwrap().clone();
    let genesis_height = genesis_block.header().height();

    let signer = InMemorySigner::from_seed("test0", KeyType::ED25519, "test0");
    let tx = SignedTransaction::from_actions(
        1,
        "test0".to_string(),
        "test0".to_string(),
        &signer,
        vec![Action::DeployContract(DeployContractAction {
            code: near_test_contracts::rs_contract().to_vec(),
        })],
        *genesis_block.hash(),
    );
    env.clients[0].process_tx(tx, false, false);
    let mut last_block = genesis_block;
    for i in 1..3 {
        last_block = env.clients[0].produce_block(i).unwrap().unwrap();
        env.process_block(0, last_block.clone(), Provenance::PRODUCED);
    }
    let query_state = |chain: &mut near_chain::Chain,
                       runtime_adapter: Arc<dyn RuntimeAdapter>,
                       account_id: AccountId| {
        let final_head = chain.store().final_head().unwrap();
        let last_final_block = chain.get_block(&final_head.last_block_hash).unwrap().clone();
        println!("{}", last_final_block.header().height());
        let response = runtime_adapter
            .query(
                0,
                &last_final_block.chunks()[0].prev_state_root(),
                last_final_block.header().height(),
                last_final_block.header().raw_timestamp(),
                &final_head.prev_block_hash,
                last_final_block.hash(),
                last_final_block.header().epoch_id(),
                &QueryRequest::ViewState { account_id, prefix: vec![].into() },
            )
            .unwrap();
        match response.kind {
            QueryResponseKind::ViewState(view_state_result) => view_state_result.values,
            // QueryResponseKind::ViewCode(code) => code.code,
            _ => panic!("Wrong return value"),
        }
    };

    let function_call_tx = SignedTransaction::from_actions(
        2,
        "test0".to_string(),
        "test0".to_string(),
        &signer,
        vec![Action::FunctionCall(FunctionCallAction {
            method_name: "write_block_height".to_string(),
            args: vec![],
            gas: 100000000000000,
            deposit: 0,
        })],
        *last_block.hash(),
    );
    env.clients[0].process_tx(function_call_tx, false, false);
    for i in 3..9 {
        last_block = env.clients[0].produce_block(i).unwrap().unwrap();
        env.process_block(0, last_block.clone(), Provenance::PRODUCED);
    }

    let runtime_adapter = env.clients[0].runtime_adapter.clone();
    let state =
        query_state(&mut env.clients[0].chain, runtime_adapter.clone(), "test0".to_string());

    env.clients[0]
        .chain
        .patch_state(&[StateRecord::Data {
            account_id: "test0".to_string(),
            data_key: from_base64(&state[0].key).unwrap(),
            value: vec![40u8],
        }])
        .unwrap();

    for i in 9..20 {
        last_block = env.clients[0].produce_block(i).unwrap().unwrap();
        env.process_block(0, last_block.clone(), Provenance::PRODUCED);
    }

    let runtime_adapter = env.clients[0].runtime_adapter.clone();
    let state2 =
        query_state(&mut env.clients[0].chain, runtime_adapter.clone(), "test0".to_string());
    println!("{:?}", state2);
}
