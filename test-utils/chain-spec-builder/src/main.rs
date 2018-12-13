extern crate clap;
extern crate network;
extern crate node_cli;
extern crate node_runtime;
extern crate serde_json;

use clap::{App, Arg};

fn main() {
    let matches = App::new("chain-spec-builder")
        .arg(
            Arg::with_name("boot_node")
                .short("b")
                .long("boot-node")
                .help(
                    "Specify a list of boot nodes in the format of \
                    '{host},{port},{test_node_index}'.")
                .multiple(true)
                .takes_value(true),
        ).get_matches();

    let boot_nodes: Vec<String> = matches
        .values_of("boot_node")
        .unwrap_or_else(clap::Values::default)
        .map(|x| {
            let d: Vec<_> = x.split(',').collect();
            if d.len() != 3 {
                let message = "boot node must be in \
                '{host},{port},{test_node_index}' format";
                panic!("{}", message);
            }
            let host = &d[0];
            let port: &str = &d[1];
            let port = port.parse::<u16>().unwrap();

            let test_node_index: &str = &d[2];
            let test_node_index = test_node_index.parse::<u32>().unwrap();
            let secret = network::service::get_test_secret_from_node_index(
                test_node_index
            );
            let key = network::test_utils::raw_key_to_peer_id(secret);

            format!("/ip4/{}/tcp/{}/p2p/{}", host, port, key.to_base58())
        })
        .collect();

    let mut chain_spec = node_runtime::test_utils::generate_test_chain_spec();
    chain_spec.boot_nodes = boot_nodes;
    let serialized = node_cli::chain_spec::serialize_chain_spec(chain_spec);
    println!("{}", serialized);
}
