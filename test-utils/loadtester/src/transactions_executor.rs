//! Executes a single transaction or a list of transactions on a set of nodes.

use crate::remote_node::RemoteNode;
use crate::transactions_generator::Generator;
use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use std::sync::{Arc, RwLock};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::timer::Interval;
use tokio::util::FutureExt;

pub struct Executor {
    /// Nodes that can be used to generate nonces
    pub nodes: Vec<Arc<RwLock<RemoteNode>>>,
}

impl Executor {
    pub fn spawn(
        nodes: Vec<Arc<RwLock<RemoteNode>>>,
        timeout: Option<Duration>,
        tps: u64,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            tokio::run(futures::lazy(move || {
                // Channels into which we can signal to send a transaction.
                let mut signal_tx = vec![];
                let all_account_ids: Vec<_> = nodes
                    .iter()
                    .map(|n| {
                        n.read()
                            .unwrap()
                            .signers
                            .iter()
                            .map(|s| s.account_id.clone())
                            .collect::<Vec<_>>()
                    })
                    .flatten()
                    .collect();

                for node in &nodes {
                    for (signer_ind, _) in node.read().unwrap().signers.iter().enumerate() {
                        let node = node.clone();
                        let all_account_ids = all_account_ids.to_vec();
                        let (tx, rx) = tokio::sync::mpsc::channel(1024);
                        signal_tx.push(tx);
                        // Spawn a task that sends transactions only from the given account making
                        // sure the nonces are correct.
                        tokio::spawn(
                            rx.map_err(|_| ())
                                .for_each(move |_| {
                                    let t =
                                        Generator::send_money(&node, signer_ind, &all_account_ids);
                                    let f = { node.write().unwrap().add_transaction(t) };
                                    f.map_err(|_| ())
                                        .timeout(Duration::from_secs(1))
                                        .map_err(|_| ())
                                })
                                .map(|_| ())
                                .map_err(|_| ()),
                        );
                    }
                }

                // Spawn the task that sets the tps.
                let interval =
                    Duration::from_nanos((Duration::from_secs(1).as_nanos() as u64) / tps);
                let timeout = timeout.map(|t| Instant::now() + t);
                let task = Interval::new_interval(interval)
                    .take_while(move |_| {
                        if let Some(t_limit) = timeout {
                            if t_limit <= Instant::now() {
                                // We hit timeout.
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    })
                    .map_err(|_| ())
                    .for_each(move |_| {
                        let ind = rand::random::<usize>() % signal_tx.len();
                        let tx = signal_tx[ind].clone();
                        tx.send(()).map(|_| ()).map_err(|_| ())
                    })
                    .map(|_| ())
                    .map_err(|_| ());

                let node = nodes[0].clone();
                let first_height = node.read().unwrap().get_current_height().map_err(|_| ());
                let last_height = node.read().unwrap().get_current_height().map_err(|_| ());
                let ft = Instant::now();
                tokio::spawn(
                    first_height
                        .and_then(|fh: u64| task.map(move |_| fh))
                        .and_then(move |fh: u64| {
                            last_height.and_then(move |lh: u64| {
                                let lt = Instant::now();
                                node.read()
                                    .unwrap()
                                    .get_transactions(fh, lh)
                                    .map(move |total_txs: u64| {
                                        println!("Start block: {}", fh);
                                        println!("End block: {}", lh);
                                        let time_passed = lt.duration_since(ft).as_secs();
                                        println!("Time passed: {} secs", time_passed);
                                        let bps = ((lh - fh + 1) as f64) / (time_passed as f64);
                                        println!("Blocks per second: {:.2}", bps);
                                        println!(
                                            "Transactions per second: {}",
                                            total_txs / time_passed
                                        );
                                        println!(
                                            "Transactions per block: {}",
                                            total_txs / (lh - fh + 1)
                                        );
                                    })
                                    .map_err(|_| ())
                            })
                        })
                        .map_err(|_| ()),
                );
                Ok(())
            }));
        })
    }
}
