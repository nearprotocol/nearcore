use actix::{Actor, Addr, System};
use futures::future::Future;
use tempdir::TempDir;

use near::config::{TESTING_INIT_BALANCE, TESTING_INIT_STAKE};
use near::{load_test_config, start_with_config, GenesisConfig, NearConfig};
use near_client::{ClientActor, Query, Status, ViewClientActor};
use near_network::test_utils::{convert_boot_nodes, open_port, WaitOrTimeout};
use near_network::NetworkClientMessages;
use near_primitives::rpc::QueryResponse;
use near_primitives::serialize::BaseEncode;
use near_primitives::test_utils::{init_integration_logger, init_test_logger};
use near_primitives::transaction::{StakeTransaction, TransactionBody};
use near_primitives::types::AccountId;
use rand::Rng;

#[derive(Clone)]
struct TestNode {
    account_id: AccountId,
    config: NearConfig,
    client: Addr<ClientActor>,
    view_client: Addr<ViewClientActor>,
}

fn init_test_staking(num_accounts: usize, num_nodes: usize, epoch_length: u64) -> Vec<TestNode> {
    init_integration_logger();

    let mut genesis_config = GenesisConfig::testing_spec(num_accounts, num_nodes);
    genesis_config.epoch_length = epoch_length;
    genesis_config.validator_kickout_threshold = 0.5;
    let first_node = open_port();

    let configs = (0..num_nodes).map(|i| {
        let mut config = load_test_config(
            &format!("near.{}", i),
            if i == 0 { first_node } else { open_port() },
            &genesis_config,
        );
        if i != 0 {
            config.network_config.boot_nodes = convert_boot_nodes(vec![("near.0", first_node)]);
        }
        config
    });
    configs
        .enumerate()
        .map(|(i, config)| {
            let dir = TempDir::new(&format!("stake_node_{}", i)).unwrap();
            let (client, view_client) = start_with_config(dir.path(), config.clone());
            TestNode { account_id: format!("near.{}", i), config, client, view_client }
        })
        .collect()
}

/// Runs one validator network, sends staking transaction for the second node and
/// waits until it becomes a validator.
#[test]
fn test_stake_nodes() {
    init_test_logger();

    let mut genesis_config = GenesisConfig::testing_spec(2, 1);
    genesis_config.epoch_length = 10;
    let first_node = open_port();
    let near1 = load_test_config("near.0", first_node, &genesis_config);
    let mut near2 = load_test_config("near.1", open_port(), &genesis_config);
    near2.network_config.boot_nodes = convert_boot_nodes(vec![("near.0", first_node)]);

    let system = System::new("NEAR");

    let dir1 = TempDir::new("sync_nodes_1").unwrap();
    let (client1, _view_client1) = start_with_config(dir1.path(), near1);
    let dir2 = TempDir::new("sync_nodes_2").unwrap();
    let (client2, _view_client2) = start_with_config(dir2.path(), near2.clone());

    let tx = TransactionBody::Stake(StakeTransaction {
        nonce: 1,
        originator: "near.1".to_string(),
        amount: 50_000_000,
        public_key: near2.block_producer.clone().unwrap().signer.public_key().to_base(),
    })
    .sign(&*near2.block_producer.clone().unwrap().signer);
    actix::spawn(client1.send(NetworkClientMessages::Transaction(tx)).map(|_| ()).map_err(|_| ()));

    WaitOrTimeout::new(
        Box::new(move |_ctx| {
            actix::spawn(client2.send(Status {}).then(|res| {
                if res.unwrap().unwrap().validators.len() == 2 {
                    System::current().stop();
                }
                futures::future::ok(())
            }));
        }),
        100,
        5000,
    )
    .start();

    system.run().unwrap();
}

#[test]
fn test_kickout() {
    let system = System::new("NEAR");
    let test_nodes = init_test_staking(4, 4, 16);
    let num_nodes = test_nodes.len();
    let mut rng = rand::thread_rng();
    let stakes = (0..num_nodes / 2).map(|_| rng.gen_range(1, 100));
    let stake_transactions = stakes.enumerate().map(|(i, stake)| {
        let test_node = &test_nodes[i];
        TransactionBody::Stake(StakeTransaction {
            nonce: 1,
            originator: test_node.account_id.clone(),
            amount: stake,
            public_key: test_node
                .config
                .block_producer
                .as_ref()
                .unwrap()
                .signer
                .public_key()
                .to_base(),
        })
        .sign(&*test_node.config.block_producer.as_ref().unwrap().signer)
    });

    for (i, stake_transaction) in stake_transactions.enumerate() {
        let test_node = &test_nodes[i];
        actix::spawn(
            test_node
                .client
                .send(NetworkClientMessages::Transaction(stake_transaction))
                .map(|_| ())
                .map_err(|_| ()),
        );
    }

    WaitOrTimeout::new(
        Box::new(move |_ctx| {
            let test_nodes = test_nodes.clone();
            let test_node1 = test_nodes[0].clone();
            actix::spawn(test_node1.client.send(Status {}).then(move |res| {
                //                info!("status result: {:?}", res);
                let expected: Vec<_> =
                    (num_nodes / 2..num_nodes).map(|i| format!("near.{}", i)).collect();
                if res.unwrap().unwrap().validators == expected {
                    for i in 0..num_nodes / 2 {
                        actix::spawn(
                            test_node1
                                .view_client
                                .send(Query {
                                    path: format!("account/{}", test_nodes[i].account_id.clone()),
                                    data: vec![],
                                })
                                .then(|res| match res.unwrap().unwrap() {
                                    QueryResponse::ViewAccount(result) => {
                                        assert_eq!(result.stake, 0);
                                        assert_eq!(
                                            result.amount,
                                            TESTING_INIT_BALANCE + TESTING_INIT_STAKE
                                        );
                                        futures::future::ok(())
                                    }
                                    _ => panic!("wrong return result"),
                                }),
                        );
                    }
                    for i in num_nodes / 2..num_nodes {
                        actix::spawn(
                            test_node1
                                .view_client
                                .send(Query {
                                    path: format!("account/{}", test_nodes[i].account_id.clone()),
                                    data: vec![],
                                })
                                .then(|res| match res.unwrap().unwrap() {
                                    QueryResponse::ViewAccount(result) => {
                                        assert_eq!(result.stake, TESTING_INIT_STAKE);
                                        assert_eq!(result.amount, TESTING_INIT_BALANCE);
                                        futures::future::ok(())
                                    }
                                    _ => panic!("wrong return result"),
                                }),
                        );
                    }
                    System::current().stop();
                }
                futures::future::ok(())
            }));
        }),
        1000,
        5000,
    )
    .start();

    system.run().unwrap();
}
