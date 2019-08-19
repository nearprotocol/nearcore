use std::sync::Arc;

use actix::{Actor, System};
use borsh::Serializable;
use futures::future::Future;

use near_jsonrpc::client::new_client;
use near_jsonrpc::test_utils::start_all;
use near_network::test_utils::{wait_or_panic, WaitOrTimeout};
use near_primitives::crypto::signer::InMemorySigner;
use near_primitives::serialize::to_base64;
use near_primitives::test_utils::init_test_logger;
use near_primitives::transaction::SignedTransaction;
use near_primitives::views::FinalTransactionStatus;

/// Test sending transaction via json rpc without waiting.
#[test]
fn test_send_tx_async() {
    init_test_logger();

    System::run(|| {
        let (_view_client_addr, addr) = start_all(true);

        let mut client = new_client(&format!("http://{}", addr));
        let signer = InMemorySigner::from_seed("test1", "test1");
        let tx = SignedTransaction::send_money(
            1,
            "test1".to_string(),
            "test2".to_string(),
            Arc::new(signer),
            100,
        );
        let tx_hash: String = (&tx.get_hash()).into();
        let tx_hash2 = tx_hash.clone();
        let bytes = tx.try_to_vec().unwrap();
        actix::spawn(
            client
                .broadcast_tx_async(to_base64(&bytes))
                .map_err(|_| ())
                .map(move |result| assert_eq!(tx_hash, result)),
        );
        WaitOrTimeout::new(
            Box::new(move |_| {
                actix::spawn(
                    client.tx(tx_hash2.clone()).map_err(|err| println!("Error: {:?}", err)).map(
                        |result| {
                            if result.status == FinalTransactionStatus::Completed {
                                System::current().stop();
                            }
                        },
                    ),
                )
            }),
            100,
            1000,
        )
        .start();
    })
    .unwrap();
}

/// Test sending transaction and waiting for it to be committed to a block.
#[test]
fn test_send_tx_commit() {
    init_test_logger();

    System::run(|| {
        let (_view_client_addr, addr) = start_all(true);

        let mut client = new_client(&format!("http://{}", addr));
        let signer = InMemorySigner::from_seed("test1", "test1");
        let tx = SignedTransaction::send_money(
            1,
            "test1".to_string(),
            "test2".to_string(),
            Arc::new(signer),
            100,
        );
        let bytes = tx.try_to_vec().unwrap();
        actix::spawn(
            client
                .broadcast_tx_commit(to_base64(&bytes))
                .map_err(|why| {
                    System::current().stop();
                    panic!(why);
                })
                .map(move |result| {
                    assert_eq!(result.status, FinalTransactionStatus::Completed);
                    System::current().stop();
                }),
        );
        wait_or_panic(10000);
    })
    .unwrap();
}
