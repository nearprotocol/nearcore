use crate::transactions_executor::Executor;
use env_logger::Builder;
use std::net::SocketAddr;
use std::str::FromStr;

pub mod remote_node;
pub mod sampler;
pub mod stats;
pub mod transactions_executor;
pub mod transactions_generator;
use node_runtime::chain_spec::{AuthorityRotation, ChainSpec, DefaultIdType};
use remote_node::RemoteNode;
use std::time::Duration;

#[allow(dead_code)]
fn configure_logging(log_level: log::LevelFilter) {
    let internal_targets = vec!["observer"];
    let mut builder = Builder::from_default_env();
    internal_targets.iter().for_each(|internal_targets| {
        builder.filter(Some(internal_targets), log_level);
    });
    builder.default_format_timestamp_nanos(true);
    builder.try_init().unwrap();
}

fn main() {
    configure_logging(log::LevelFilter::Debug);
    let (chain_spec, _) = ChainSpec::testing_spec(
        DefaultIdType::Enumerated,
        10,
        2,
        AuthorityRotation::ProofOfAuthority,
    );
    let accounts: Vec<_> = chain_spec.accounts.into_iter().map(|t| t.0).collect();

    let addrs = ["127.0.0.1:3030", "127.0.0.1:3031"];

    let num_nodes = addrs.len();
    let accounts_per_node = accounts.len() / num_nodes;
    let mut nodes = vec![];
    for (i, addr) in addrs.iter().enumerate() {
        let node = RemoteNode::new(
            SocketAddr::from_str(addr).unwrap(),
            &accounts[(i * accounts_per_node)..((i + 1) * accounts_per_node)],
        );
        nodes.push(node);
    }

    // Start the executor.
    let handle = Executor::spawn(nodes, Some(Duration::from_secs(10)), 1600);
    handle.join().unwrap();
}
