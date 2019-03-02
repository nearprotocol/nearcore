use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use futures::future;
use futures::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc::{Receiver, Sender};
use log::{debug, error, info, warn};
use tokio::{self, timer::Interval};

use nightshade::nightshade_task::Control;
use primitives::aggregate_signature::BlsPublicKey;
use primitives::chain::{PayloadRequest, PayloadResponse};
use primitives::hash::CryptoHash;
use primitives::signature::PublicKey;
use primitives::types::AuthorityId;

use crate::Pool;
use crate::tx_gossip::TxGossip;

#[derive(Clone, Debug)]
pub enum MemPoolControl {
    Reset {
        authority_id: AuthorityId,
        num_authorities: usize,

        block_index: u64,
        public_keys: Vec<PublicKey>,
        bls_public_keys: Vec<BlsPublicKey>,
    },
    Stop,
}

pub fn spawn_pool(
    pool: Arc<Pool>,
    mempool_control_rx: Receiver<MemPoolControl>,
    control_tx: Sender<Control>,
    retrieve_payload_rx: Receiver<(AuthorityId, CryptoHash)>,
    payload_request_tx: Sender<PayloadRequest>,
    payload_response_rx: Receiver<PayloadResponse>,
    inc_tx_gossip_rx: Receiver<TxGossip>,
    out_tx_gossip_tx: Sender<TxGossip>,
    gossip_tx_period: Duration,
) {
    // Handle request from NightshadeTask for confirmation on a payload.
    // If the payload can't be built from the mempool task to fetch necessary data is spawned and the
    // request is stored until it is ready.
    let pool1 = pool.clone();
    let control_tx1 = control_tx.clone();
    let task = retrieve_payload_rx.for_each(move |(authority_id, hash)| {
        info!(
            target: "mempool",
            "Payload confirmation for {} from {}, authority_id={:?}",
            hash,
            authority_id,
            pool1.authority_id.read().expect(crate::POISONED_LOCK_ERR).deref(),
        );
        if !pool1.contains_payload_snapshot(&hash) {
            tokio::spawn(
                payload_request_tx
                    .clone()
                    .send(PayloadRequest::BlockProposal(authority_id, hash))
                    .map(|_| ())
                    .map_err(|e| warn!(target: "mempool", "Error sending message: {}", e)),
            );

            pool1.add_pending(authority_id, hash);
        } else {
            let send_confirmation = control_tx1
                .clone()
                .send(Control::PayloadConfirmation(authority_id, hash))
                .map(|_| ())
                .map_err(
                    |_| error!(target: "mempool", "Fail sending control signal to nightshade"),
                );
            tokio::spawn(send_confirmation);
        }
        future::ok(())
    });
    tokio::spawn(task);

    // Receive payload and send confirmation signal of unblocked payloads.
    let pool3 = pool.clone();
    let control_tx2 = control_tx.clone();
    let task = payload_response_rx.for_each(move |payload_response| {
        if let Err(e) = match payload_response {
            PayloadResponse::General(payload) => pool3.add_payload(payload),
            PayloadResponse::BlockProposal(authority_id, payload) => {
                pool3.add_payload_snapshot(authority_id, payload)
            }
        } {
            warn!(target: "mempool", "Failed to add incoming payload: {}", e);
        }

        for (authority_id, hash) in pool3.ready_snapshots() {
            let send_confirmation = control_tx2
                .clone()
                .send(Control::PayloadConfirmation(authority_id, hash))
                .map(|_| ())
                .map_err(
                    |_| error!(target: "mempool", "Fail sending control signal to nightshade"),
                );
            tokio::spawn(send_confirmation);
        }

        future::ok(())
    });
    tokio::spawn(task);

    let pool4 = pool.clone();
    let task = mempool_control_rx.for_each(move |control| {
        pool4.reset(control.clone());
        let ns_control = match control {
            MemPoolControl::Reset {
                authority_id,
                block_index,
                public_keys,
                bls_public_keys,
                ..
            } => {
                let hash = pool4.snapshot_payload();
                debug!(
                    target: "mempool",
                    "[{:?}] Mempool snapshot hash: {:?}",
                    pool4.authority_id.read().expect(crate::POISONED_LOCK_ERR),
                    hash
                );
                Control::Reset {
                    owner_uid: authority_id,
                    block_index,
                    hash,
                    public_keys,
                    bls_public_keys,
                }
            }
            MemPoolControl::Stop => Control::Stop,
        };
        tokio::spawn(
            control_tx
                .clone()
                .send(ns_control)
                .map(|_| ())
                .map_err(|e| error!(target: "mempool", "Failed to send NS control: {}", e)),
        );
        future::ok(())
    });
    tokio::spawn(task);

    // Receive transaction gossips
    let pool5 = pool.clone();
    let task = inc_tx_gossip_rx.for_each(move |tx_gossip| {
        // TODO: verify signature
        if let Err(e) = pool5.add_payload_with_author(tx_gossip.payload, tx_gossip.sender_id) {
            warn!(target: "pool", "Failed to add payload from tx gossip: {}", e);
        }

        future::ok(())
    });
    tokio::spawn(task);

    // Make announcements of new payloads created from this node.
    let pool6 = pool.clone();
    let task = Interval::new_interval(gossip_tx_period)
        .for_each(move |_| {
            for tx_gossip in pool6.prepare_payload_announce() {
                tokio::spawn(
                    out_tx_gossip_tx
                        .clone()
                        .send(tx_gossip)
                        .map(|_| ())
                        .map_err(|e| warn!(target: "pool", "Error sending message: {}", e)),
                );
            }
            future::ok(())
        })
        .map_err(|e| error!(target: "pool", "Timer error: {}", e));
    tokio::spawn(task);
}
