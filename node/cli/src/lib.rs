extern crate beacon;
extern crate beacon_chain_handler;
extern crate chain;
extern crate clap;
extern crate env_logger;
extern crate futures;
#[macro_use]
extern crate log;
extern crate network;
extern crate node_rpc;
extern crate node_runtime;
extern crate parking_lot;
extern crate primitives;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[cfg_attr(test, macro_use)]
extern crate serde_json;
extern crate shard;
extern crate storage;
extern crate tokio;

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{App, Arg};
use env_logger::Builder;
use futures::future;
use futures::sync::mpsc::{channel, Receiver, Sender};
use std::net::{Ipv4Addr, IpAddr, SocketAddr};
use parking_lot::{Mutex, RwLock};

use beacon::types::{BeaconBlockChain, SignedBeaconBlock, SignedBeaconBlockHeader};
use beacon_chain_handler::producer::ChainConsensusBlockBody;
use chain::SignedBlock;
use network::protocol::{Protocol, ProtocolConfig};
use node_rpc::api::RpcImpl;
use node_runtime::{state_viewer::StateDbViewer, Runtime};
use primitives::signer::InMemorySigner;
use primitives::types::SignedTransaction;
use shard::{ShardBlockChain, SignedShardBlock};
use storage::{StateDb, Storage};
use network::service::get_multiaddr;
use primitives::types::BeaconChainPayload;

pub mod chain_spec;
pub mod test_utils;

fn get_storage(base_path: &Path) -> Arc<Storage> {
    let mut storage_path = base_path.to_owned();
    storage_path.push("storage/db");
    match fs::canonicalize(storage_path.clone()) {
        Ok(path) => info!("Opening storage database at {:?}", path),
        _ => info!("Could not resolve {:?} path", storage_path),
    };
    Arc::new(storage::open_database(&storage_path.to_string_lossy()))
}

fn spawn_rpc_server_task(
    transactions_tx: Sender<SignedTransaction>,
    rpc_port: Option<u16>,
    shard_chain: Arc<ShardBlockChain>,
    state_db: Arc<StateDb>,
) {
    let state_db_viewer = StateDbViewer::new(shard_chain, state_db);
    let rpc_impl = RpcImpl::new(state_db_viewer, transactions_tx);
    let rpc_handler = node_rpc::api::get_handler(rpc_impl);
    let rpc_port = rpc_port.unwrap_or(3030);
    let rpc_addr = Some(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), rpc_port)
    );
    let server = node_rpc::server::get_server(rpc_handler, rpc_addr);
    tokio::spawn(future::lazy(|| {
        server.wait();
        Ok(())
    }));
}

fn spawn_network_tasks(
    p2p_port: Option<u16>,
    test_node_index: Option<u32>,
    beacon_chain: Arc<BeaconBlockChain>,
    beacon_block_tx: Sender<SignedBeaconBlock>,
    transactions_tx: Sender<SignedTransaction>,
) {
    let (receipts_tx, _receipts_rx) = channel(1024);
    let (net_messages_tx, net_messages_rx) = channel(1024);
    let protocol_config = ProtocolConfig::default();
    let protocol = Protocol::<_, SignedBeaconBlockHeader, BeaconChainPayload>::new(
        protocol_config,
        beacon_chain,
        beacon_block_tx,
        transactions_tx,
        receipts_tx,
        net_messages_tx,
    );
    let mut network_config = network::service::NetworkConfiguration::new();
    let p2p_port = p2p_port.unwrap_or(30333);
    network_config.listen_addresses = vec![
        get_multiaddr(Ipv4Addr::UNSPECIFIED, p2p_port),
    ];

    network_config.use_secret = test_node_index
        .map(network::service::get_test_secret_from_node_index);

    let network_service = network::service::new_network_service(
        &protocol_config,
        network_config,
    );
    network::service::spawn_network_tasks(
        Arc::new(Mutex::new(network_service)),
        protocol,
        net_messages_rx,
    );
}

pub struct ServiceConfig {
    pub base_path: PathBuf,
    pub chain_spec_path: Option<PathBuf>,
    pub p2p_port: Option<u16>,
    pub rpc_port: Option<u16>,
    pub test_node_index: Option<u32>,
}

pub fn start_service<S>(config: ServiceConfig, spawn_consensus_task_fn: S)
where
    S: Fn(Receiver<SignedTransaction>, Sender<ChainConsensusBlockBody>) -> ()
        + Send
        + Sync
        + 'static,
{
    let mut builder = Builder::new();
    builder.filter(Some("runtime"), log::LevelFilter::Debug);
    builder.filter(None, log::LevelFilter::Info);
    builder.init();

    // Create shared-state objects.
    let storage = get_storage(&config.base_path);
    let chain_spec = chain_spec::read_or_default_chain_spec(&config.chain_spec_path);

    let state_db = Arc::new(StateDb::new(storage.clone()));
    let runtime = Arc::new(RwLock::new(Runtime::new(state_db.clone())));
    let genesis_root = runtime.write().apply_genesis_state(
        &chain_spec.accounts,
        &chain_spec.genesis_wasm,
        &chain_spec.initial_authorities,
    );

    let shard_genesis = SignedShardBlock::genesis(genesis_root);
    let genesis = SignedBeaconBlock::genesis(shard_genesis.block_hash());
    let shard_chain = Arc::new(ShardBlockChain::new(shard_genesis, storage.clone()));
    let beacon_chain = Arc::new(BeaconBlockChain::new(genesis, storage.clone()));
    let signer = Arc::new(InMemorySigner::default());

    tokio::run(future::lazy(move || {
        // TODO: TxFlow should be listening on these transactions.
        let (transactions_tx, transactions_rx) = channel(1024);
        spawn_rpc_server_task(
            transactions_tx.clone(),
            config.rpc_port,
            shard_chain.clone(),
            state_db.clone(),
        );

        // Create a task that consumes the consensuses
        // and produces the beacon chain blocks.
        let (beacon_block_consensus_body_tx, beacon_block_consensus_body_rx) = channel(1024);
        beacon_chain_handler::producer::spawn_block_producer(
            beacon_chain.clone(),
            shard_chain.clone(),
            runtime.clone(),
            signer.clone(),
            state_db.clone(),
            beacon_block_consensus_body_rx,
        );

        // Create task that can import beacon chain blocks from other peers.
        let (beacon_block_tx, beacon_block_rx) = channel(1024);
        beacon_chain_handler::importer::spawn_block_importer(
            beacon_chain.clone(),
            shard_chain.clone(),
            runtime.clone(),
            state_db.clone(),
            beacon_block_rx,
        );

        // Spawn protocol and the network_task.
        // Note, that network and RPC are using the same channels
        // to send transactions and receipts for processing.
        spawn_network_tasks(
            config.p2p_port,
            config.test_node_index,
            beacon_chain.clone(),
            beacon_block_tx.clone(),
            transactions_tx.clone(),
        );

        spawn_consensus_task_fn(
            transactions_rx,
            beacon_block_consensus_body_tx,
        );
        Ok(())
    }));
}

pub fn run() {
    let matches = App::new("near")
        .arg(
            Arg::with_name("base_path")
                .short("b")
                .long("base-path")
                .value_name("PATH")
                .help("Sets a base path for persisted files.")
                .takes_value(true),
        ).arg(
            Arg::with_name("chain_spec_file")
                .short("c")
                .long("chain-spec-file")
                .value_name("CHAIN_SPEC_FILE")
                .help("Sets a file location to read a custom chain spec.")
                .takes_value(true),
        ).arg(
            Arg::with_name("p2p_port")
                .short("p")
                .long("p2p_port")
                .value_name("P2P_PORT")
                .help("Sets the p2p protocol TCP port.")
                .takes_value(true),
        ).arg(
            Arg::with_name("rpc_port")
                .short("r")
                .long("rpc_port")
                .value_name("RPC_PORT")
                .help("Sets the rpc protocol TCP port.")
                .takes_value(true),
        ).arg(
            Arg::with_name("test_node_index")
                .long("test-node-index")
                .value_name("TEST_NODE_INDEX")
                .help(
                    "Used as a seed for generating a node ID.\
                     This should only be used for deterministically \
                     creating node ID's during tests.",
                )
                .takes_value(true),
        ).get_matches();

    let base_path = matches
        .value_of("base_path")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let chain_spec_path = matches
        .value_of("chain_spec_file")
        .map(PathBuf::from);

    let p2p_port = matches
        .value_of("p2p_port")
        .map(|x| x.parse::<u16>().unwrap());

    let rpc_port = matches
        .value_of("rpc_port")
        .map(|x| x.parse::<u16>().unwrap());

    let test_node_index = matches
        .value_of("test_node_index")
        .map(|x| x.parse::<u32>().unwrap());

    let config = ServiceConfig {
        base_path,
        chain_spec_path,
        p2p_port,
        rpc_port,
        test_node_index,
    };
    start_service(config, test_utils::spawn_pasthrough_consensus);
}
