extern crate env_logger;
#[macro_use]
extern crate log;
extern crate serde;
extern crate serde_derive;

use std::sync::Arc;

use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc;

use client::Client;
use configs::{ClientConfig, NetworkConfig, RPCConfig, get_testnet_configs};
use network::spawn_network;
use nightshade::nightshade_task::{Control, spawn_nightshade_task};
use primitives::types::AccountId;
use coroutines::ns_control_builder::get_control;
use coroutines::ns_producer::spawn_block_producer;

#[cfg(test)]
pub mod testing_utils;

pub fn start_from_client(
    client: Arc<Client>,
    account_id: Option<AccountId>,
    network_cfg: NetworkConfig,
    _rpc_cfg: RPCConfig,
) {
    let node_task = futures::lazy(move || {
        // Create control channel and send kick-off reset signal.
        let (control_tx, control_rx) = mpsc::channel(1024);

        // Launch block syncing / importing.
        let (inc_block_tx, _inc_block_rx) = mpsc::channel(1024);
        let (_out_block_tx, out_block_rx) = mpsc::channel(1024);

        // Launch Nightshade task
        let (inc_gossip_tx, inc_gossip_rx) = mpsc::channel(1024);
        let (out_gossip_tx, out_gossip_rx) = mpsc::channel(1024);
        let (consensus_tx, consensus_rx) = mpsc::channel(1024);

        spawn_nightshade_task(inc_gossip_rx, out_gossip_tx, consensus_tx, control_rx);
        let start_task = control_tx
            .clone()
            .send(get_control(&client, 1))
            .map(|_| ())
            .map_err(|e| error!("Error sending control {:?}", e));
        tokio::spawn(start_task);
        spawn_block_producer(client.clone(), consensus_rx, control_tx);

        // Launch Network task.
        spawn_network(
            account_id,
            network_cfg,
            client.clone(),
            inc_gossip_tx,
            out_gossip_rx,
            inc_block_tx,
            out_block_rx,
        );

        Ok(())
    });

    tokio::run(node_task);
}

pub fn start_from_configs(
    client_cfg: ClientConfig,
    network_cfg: NetworkConfig,
    rpc_cfg: RPCConfig
) {
    let client = Arc::new(Client::new(&client_cfg));
    start_from_client(client, Some(client_cfg.account_id), network_cfg, rpc_cfg)
}

pub fn start() {
    let (client_cfg, network_cfg, rpc_cfg) = get_testnet_configs();
    start_from_configs(client_cfg, network_cfg, rpc_cfg);
}

#[cfg(test)]
mod tests {
    use crate::testing_utils::{Node, configure_chain_spec};

    /// Creates two nodes, one boot node and secondary node booting from it. Waits until they connect.
    #[test]
    fn two_nodes() {
        let chain_spec = configure_chain_spec();
        // Create boot node.
        let alice = Node::new("t1_alice", "alice.near", 1, "127.0.0.1:3000", 3030, vec![], chain_spec.clone());
        // Create secondary node that boots from the alice node.
        let bob = Node::new("t1_bob", "bob.near", 2, "127.0.0.1:3001", 3031, vec![alice.node_info.clone()], chain_spec);

        alice.start();
        bob.start();
    }
}
