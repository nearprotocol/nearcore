use actix::{Actor, Addr, System};
use futures::future::Future;
use tempdir::TempDir;

use near::{load_test_config, start_with_config, GenesisConfig};
use near_client::{ClientActor, GetBlock, ViewClientActor};
use near_network::test_utils::{convert_boot_nodes, open_port, WaitOrTimeout};
use near_primitives::test_utils::{heavy_test, init_integration_logger};
use near_primitives::types::{BlockIndex, ShardId};
use testlib::start_nodes;

fn run_nodes(
    num_shards: usize,
    num_nodes: usize,
    num_validators: usize,
    epoch_length: BlockIndex,
    num_blocks: BlockIndex,
) {
    let system = System::new("NEAR");
    let dirs = (0..num_nodes)
        .map(|i| {
            TempDir::new(&format!("run_nodes_{}_{}_{}", num_nodes, num_validators, i)).unwrap()
        })
        .collect::<Vec<_>>();
    let clients = start_nodes(num_shards, &dirs, num_validators, epoch_length);
    let view_client = clients[clients.len() - 1].1.clone();
    WaitOrTimeout::new(
        Box::new(move |_ctx| {
            actix::spawn(view_client.send(GetBlock::Best).then(move |res| {
                match &res {
                    Ok(Ok(b))
                        if b.header.height > num_blocks && b.header.total_weight > num_blocks =>
                    {
                        System::current().stop()
                    }
                    Err(_) => return futures::future::err(()),
                    _ => {}
                };
                futures::future::ok(())
            }));
        }),
        100,
        60000,
    )
    .start();

    system.run().unwrap();
}

/// Runs two nodes that should produce blocks one after another.
#[test]
fn run_nodes_1_2_2() {
    heavy_test(|| {
        run_nodes(1, 2, 2, 10, 30);
    });
}

/// Runs two nodes, where only one is a validator.
#[test]
fn run_nodes_1_2_1() {
    heavy_test(|| {
        run_nodes(1, 2, 1, 10, 30);
    });
}

/// Runs 4 nodes that should produce blocks one after another.
#[test]
fn run_nodes_1_4_4() {
    heavy_test(|| {
        run_nodes(1, 4, 4, 8, 32);
    });
}

/// Run 4 nodes, 4 shards, 2 validators, other two track 2 shards.
#[test]
fn run_nodes_4_4_2() {
    heavy_test(|| {
        run_nodes(4, 4, 2, 8, 32);
    });
}
