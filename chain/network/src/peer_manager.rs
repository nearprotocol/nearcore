use std::cmp;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use actix::actors::resolver::{ConnectAddr, Resolver};
use actix::io::FramedWrite;
use actix::prelude::Stream;
use actix::{
    Actor, ActorFuture, Addr, AsyncContext, Context, ContextFutureSpawner, Handler, Recipient,
    StreamHandler, SystemService, WrapFuture,
};
use chrono::offset::TimeZone;
use chrono::{DateTime, Utc};
use futures::future;
use log::{debug, error, info, trace, warn};
use rand::{thread_rng, Rng};
use tokio::codec::FramedRead;
use tokio::io::AsyncRead;
use tokio::net::{TcpListener, TcpStream};

use near_primitives::utils::from_timestamp;
use near_store::Store;

use crate::codec::Codec;
use crate::peer::Peer;
use crate::peer_store::PeerStore;
use crate::routing::{Edge, EdgeInfo, EdgeType, RoutingTable};
use crate::types::{
    AccountOrPeerId, AnnounceAccount, Ban, Consolidate, ConsolidateResponse, FullPeerInfo,
    InboundTcpConnect, KnownPeerStatus, NetworkInfo, OutboundTcpConnect, PeerId, PeerList,
    PeerManagerRequest, PeerMessage, PeerType, PeersRequest, PeersResponse, Ping, Pong,
    QueryPeerStats, RawRoutedMessage, ReasonForBan, RoutedMessage, RoutedMessageBody, SendMessage,
    SyncData, Unregister,
};
use crate::types::{
    NetworkClientMessages, NetworkConfig, NetworkRequests, NetworkResponses, PeerInfo,
};
use near_primitives::types::AccountId;

/// How often to request peers from active peers.
const REQUEST_PEERS_SECS: i64 = 60;

macro_rules! unwrap_or_error(($obj: expr, $error: expr) => (match $obj {
    Ok(result) => result,
    Err(err) => {
        error!(target: "network", "{}: {}", $error, err);
        return;
    }
}));

/// Contains information relevant to an active peer.
struct ActivePeer {
    addr: Addr<Peer>,
    full_peer_info: FullPeerInfo,
    /// Number of bytes we've received from the peer.
    received_bytes_per_sec: u64,
    /// Number of bytes we've sent to the peer.
    sent_bytes_per_sec: u64,
    /// Last time requested peers.
    last_time_peer_requested: DateTime<Utc>,
}

/// Actor that manages peers connections.
pub struct PeerManagerActor {
    /// Networking configuration.
    config: NetworkConfig,
    /// Peer information for this node.
    peer_id: PeerId,
    /// Address of the client actor.
    client_addr: Recipient<NetworkClientMessages>,
    /// Peer store that provides read/write access to peers.
    peer_store: PeerStore,
    /// Set of outbound connections that were not consolidated yet.
    outgoing_peers: HashSet<PeerId>,
    /// Active peers (inbound and outbound) with their full peer information.
    active_peers: HashMap<PeerId, ActivePeer>,
    /// Routing table to keep track of account id
    routing_table: RoutingTable,
    /// Monitor peers attempts, used for fast checking in the beginning with exponential backoff.
    monitor_peers_attempts: u64,
}

impl PeerManagerActor {
    pub fn new(
        store: Arc<Store>,
        config: NetworkConfig,
        client_addr: Recipient<NetworkClientMessages>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let peer_store = PeerStore::new(store, &config.boot_nodes)?;
        debug!(target: "network", "Found known peers: {} (boot nodes={})", peer_store.len(), config.boot_nodes.len());

        let me = config.public_key.clone().into();
        Ok(PeerManagerActor {
            peer_id: config.public_key.clone().into(),
            config,
            client_addr,
            peer_store,
            active_peers: HashMap::default(),
            outgoing_peers: HashSet::default(),
            routing_table: RoutingTable::new(me),
            monitor_peers_attempts: 0,
        })
    }

    fn num_active_peers(&self) -> usize {
        self.active_peers.len()
    }

    /// Register a direct connection to a new peer. This will be called after successfully
    /// establishing a connection with another peer. It become part of the active peers.
    ///
    /// To build new edge between this pair of nodes both signatures are required.
    /// Signature from this node is passed in `edge_info`
    /// Signature from the other node is passed in `full_peer_info.edge_info`.
    fn register_peer(
        &mut self,
        full_peer_info: FullPeerInfo,
        edge_info: EdgeInfo,
        addr: Addr<Peer>,
        ctx: &mut Context<Self>,
    ) {
        if self.outgoing_peers.contains(&full_peer_info.peer_info.id) {
            self.outgoing_peers.remove(&full_peer_info.peer_info.id);
        }
        unwrap_or_error!(
            self.peer_store.peer_connected(&full_peer_info),
            "Failed to save peer data"
        );

        let new_edge = Edge::new(
            self.peer_id.clone(),                // source
            full_peer_info.peer_info.id.clone(), // target
            edge_info.nonce,
            EdgeType::Added,
            Some(edge_info.signature),
            Some(full_peer_info.edge_info.signature.clone()),
        );

        self.active_peers.insert(
            full_peer_info.peer_info.id.clone(),
            ActivePeer {
                addr: addr.clone(),
                full_peer_info,
                sent_bytes_per_sec: 0,
                received_bytes_per_sec: 0,
                last_time_peer_requested: Utc.timestamp(0, 0),
            },
        );

        assert!(self.routing_table.process_edge(new_edge.clone()));

        // TODO(MarX, #1363): Implement sync service. Right now all edges and known validators
        //  are sent during handshake.
        let known_edges = self.routing_table.get_edges();
        let routing_table_info = self.routing_table.info();
        let wait_for_sync = 1;

        // Start syncing network point of view. Wait until both parties are connected before start
        // sending messages.
        ctx.run_later(Duration::from_secs(wait_for_sync), move |act, ctx| {
            let _ = addr.do_send(SendMessage {
                message: PeerMessage::Sync(SyncData {
                    edges: known_edges,
                    known_accounts: routing_table_info.account_peers,
                }),
            });

            // TODO(MarX): Only broadcast new message from the inbound connection.
            // Wait a time out before broadcasting this new edge to let the other party finish handshake.
            act.broadcast_message(
                ctx,
                SendMessage { message: PeerMessage::Sync(SyncData::edge(new_edge)) },
            );
        });
    }

    /// Remove a peer from the active peer set. If the peer doesn't belong to the active peer set
    /// data from ongoing connection established is removed.
    fn unregister_peer(&mut self, peer_id: PeerId) {
        // If this is an unconsolidated peer because failed / connected inbound, just delete it.
        if self.outgoing_peers.contains(&peer_id) {
            self.outgoing_peers.remove(&peer_id);
            return;
        }
        self.active_peers.remove(&peer_id);

        // TODO(MarX, #1312): Trigger actions after a peer is removed successfully regarding networking
        //  (Write specification about the actions)

        unwrap_or_error!(self.peer_store.peer_disconnected(&peer_id), "Failed to save peer data");
    }

    /// Add peer to ban list.
    fn ban_peer(&mut self, peer_id: &PeerId, ban_reason: ReasonForBan) {
        info!(target: "network", "Banning peer {:?} for {:?}", peer_id, ban_reason);
        self.active_peers.remove(&peer_id);
        unwrap_or_error!(self.peer_store.peer_ban(peer_id, ban_reason), "Failed to save peer data");
    }

    /// Connects peer with given TcpStream and optional information if it's outbound.
    fn connect_peer(
        &mut self,
        recipient: Addr<Self>,
        stream: TcpStream,
        peer_type: PeerType,
        peer_info: Option<PeerInfo>,
        edge_info: Option<EdgeInfo>,
    ) {
        let peer_id = self.peer_id.clone();
        let account_id = self.config.account_id.clone();
        let server_addr = self.config.addr;
        let handshake_timeout = self.config.handshake_timeout;
        let client_addr = self.client_addr.clone();
        Peer::create(move |ctx| {
            let server_addr = server_addr.unwrap_or_else(|| stream.local_addr().unwrap());
            let remote_addr = stream.peer_addr().unwrap();
            let (read, write) = stream.split();

            // TODO: check if peer is banned or known based on IP address and port.

            Peer::add_stream(FramedRead::new(read, Codec::new()), ctx);
            Peer::new(
                PeerInfo { id: peer_id, addr: Some(server_addr), account_id },
                remote_addr,
                peer_info,
                peer_type,
                FramedWrite::new(write, Codec::new(), ctx),
                handshake_timeout,
                recipient,
                client_addr,
                edge_info,
            )
        });
    }

    fn is_outbound_bootstrap_needed(&self) -> bool {
        (self.active_peers.len() + self.outgoing_peers.len())
            < (self.config.peer_max_count as usize)
    }

    /// Returns single random peer with the most weight.
    fn most_weight_peers(&self) -> Vec<FullPeerInfo> {
        let max_weight = match self
            .active_peers
            .values()
            .map(|active_peer| active_peer.full_peer_info.chain_info.total_weight)
            .max()
        {
            Some(w) => w,
            None => {
                return vec![];
            }
        };
        self.active_peers
            .values()
            .filter_map(|active_peer| {
                if active_peer.full_peer_info.chain_info.total_weight == max_weight {
                    Some(active_peer.full_peer_info.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    }

    /// Returns bytes sent/received across all peers.
    fn get_total_bytes_per_sec(&self) -> (u64, u64) {
        let sent_bps = self.active_peers.values().map(|x| x.sent_bytes_per_sec).sum();
        let received_bps = self.active_peers.values().map(|x| x.received_bytes_per_sec).sum();
        (sent_bps, received_bps)
    }

    /// Get a random peer we are not connected to from the known list.
    fn sample_random_peer(&self, ignore_list: &HashSet<PeerId>) -> Option<PeerInfo> {
        let unconnected_peers = self.peer_store.unconnected_peers(ignore_list);
        let index = thread_rng().gen_range(0, std::cmp::max(unconnected_peers.len(), 1));

        unconnected_peers
            .iter()
            .enumerate()
            .filter_map(|(i, v)| if i == index { Some(v.clone()) } else { None })
            .next()
    }

    /// Query current peers for more peers.
    fn query_active_peers_for_more_peers(&mut self, ctx: &mut Context<Self>) {
        let mut requests = vec![];
        let msg = SendMessage { message: PeerMessage::PeersRequest };
        for (_, active_peer) in self.active_peers.iter_mut() {
            if Utc::now().signed_duration_since(active_peer.last_time_peer_requested).num_seconds()
                > REQUEST_PEERS_SECS
            {
                active_peer.last_time_peer_requested = Utc::now();
                requests.push(active_peer.addr.send(msg.clone()));
            }
        }
        future::join_all(requests)
            .into_actor(self)
            .map_err(|e, _, _| error!("Failed sending broadcast message: {}", e))
            .and_then(|_, _, _| actix::fut::ok(()))
            .spawn(ctx);
    }

    /// Periodically query peer actors for latest weight and traffic info.
    fn monitor_peer_stats(&mut self, ctx: &mut Context<Self>) {
        for (peer_id, active_peer) in self.active_peers.iter() {
            let peer_id1 = peer_id.clone();
            active_peer.addr.send(QueryPeerStats {})
                .into_actor(self)
                .map_err(|err, _, _| error!("Failed sending message: {}", err))
                .and_then(move |res, act, _| {
                    if res.is_abusive {
                        warn!(target: "network", "Banning peer {} for abuse ({} sent, {} recv)", peer_id1, res.message_counts.0, res.message_counts.1);
                        act.ban_peer(&peer_id1, ReasonForBan::Abusive);
                    } else if let Some(active_peer) = act.active_peers.get_mut(&peer_id1) {
                        active_peer.full_peer_info.chain_info = res.chain_info;
                        active_peer.sent_bytes_per_sec = res.sent_bytes_per_sec;
                        active_peer.received_bytes_per_sec = res.received_bytes_per_sec;
                    }
                    actix::fut::ok(())
                })
                .spawn(ctx);
        }

        ctx.run_later(self.config.peer_stats_period, move |act, ctx| {
            act.monitor_peer_stats(ctx);
        });
    }

    /// Periodically monitor list of peers and:
    ///  - request new peers from connected peers,
    ///  - bootstrap outbound connections from known peers,
    ///  - unban peers that have been banned for awhile,
    ///  - remove expired peers,
    fn monitor_peers(&mut self, ctx: &mut Context<Self>) {
        let mut to_unban = vec![];
        for (peer_id, peer_state) in self.peer_store.iter() {
            if let KnownPeerStatus::Banned(_, last_banned) = peer_state.status {
                let interval = unwrap_or_error!(
                    (Utc::now() - from_timestamp(last_banned)).to_std(),
                    "Failed to convert time"
                );
                if interval > self.config.ban_window {
                    info!(target: "network", "Monitor peers: unbanned {} after {:?}.", peer_id, interval);
                    to_unban.push(peer_id.clone());
                }
            }
        }
        for peer_id in to_unban {
            unwrap_or_error!(self.peer_store.peer_unban(&peer_id), "Failed to unban a peer");
        }

        if self.is_outbound_bootstrap_needed() {
            if let Some(peer_info) = self.sample_random_peer(&self.outgoing_peers) {
                self.outgoing_peers.insert(peer_info.id.clone());
                ctx.notify(OutboundTcpConnect { peer_info });
            } else {
                self.query_active_peers_for_more_peers(ctx);
            }
        }

        unwrap_or_error!(
            self.peer_store.remove_expired(&self.config),
            "Failed to remove expired peers"
        );

        // Reschedule the bootstrap peer task, starting of as quick as possible with exponential backoff.
        let wait = Duration::from_millis(cmp::min(
            self.config.bootstrap_peers_period.as_millis() as u64,
            10 << self.monitor_peers_attempts,
        ));
        self.monitor_peers_attempts = cmp::min(13, self.monitor_peers_attempts + 1);
        ctx.run_later(wait, move |act, ctx| {
            act.monitor_peers(ctx);
        });
    }

    /// Broadcast message to all active peers.
    fn broadcast_message(&self, ctx: &mut Context<Self>, msg: SendMessage) {
        // TODO(MarX, #1363): Implement smart broadcasting. (MST)

        let requests: Vec<_> =
            self.active_peers.values().map(|peer| peer.addr.send(msg.clone())).collect();

        future::join_all(requests)
            .into_actor(self)
            .map_err(|e, _, _| error!("Failed sending broadcast message: {}", e))
            .and_then(|_, _, _| actix::fut::ok(()))
            .spawn(ctx);
    }

    fn announce_account(&mut self, ctx: &mut Context<Self>, announce_account: AnnounceAccount) {
        if self
            .routing_table
            .add_account(announce_account.account_id.clone(), announce_account.peer_id.clone())
        {
            self.broadcast_message(
                ctx,
                SendMessage { message: PeerMessage::AnnounceAccount(announce_account) },
            );
        }
    }

    /// Send message to peer that belong to our active set
    fn send_message(&mut self, ctx: &mut Context<Self>, peer_id: &PeerId, message: PeerMessage) {
        if let Some(active_peer) = self.active_peers.get(&peer_id) {
            active_peer
                .addr
                .send(SendMessage { message })
                .into_actor(self)
                .map_err(|e, _, _| error!("Failed sending message: {}", e))
                .and_then(|_, _, _| actix::fut::ok(()))
                .spawn(ctx);
        } else {
            // TODO(MarX): This should be unreachable! Probably it is reaching this point because
            //  the peer is added to the routing table before being added to the set of active peers.
            error!(target: "network",
                   "Sending message to: {} (which is not an active peer) Active Peers: {:?}\n{:?}",
                   peer_id,
                   self.active_peers.keys(),
                   message
            );
        }
    }

    /// Route message to target peer.
    fn send_message_to_peer(&mut self, ctx: &mut Context<Self>, msg: RoutedMessage) {
        match self.routing_table.find_route(&msg.target) {
            Ok(peer_id) => {
                self.send_message(ctx, &peer_id, PeerMessage::Routed(msg));
            }
            Err(find_route_error) => {
                // TODO(MarX, #1369): Message is dropped here. Define policy for this case.
                warn!(target: "network", "Drop message to {} Reason {:?}. Known peers: {:?} Message {:?}",
                      msg.target,
                      find_route_error,
                      self.routing_table.peer_forwarding.keys(),
                      msg,
                );
            }
        }
    }

    /// Send message to specific account.
    fn send_message_to_account(
        &mut self,
        ctx: &mut Context<Self>,
        account_id: &AccountId,
        msg: RoutedMessageBody,
    ) {
        let target = match self.routing_table.account_owner(&account_id) {
            Ok(peer_id) => peer_id,
            Err(find_route_error) => {
                // TODO(MarX, #1369): Message is dropped here. Define policy for this case.
                warn!(target: "network", "Drop message to {} Reason {:?}. Known peers: {:?} Message {:?}",
                      account_id,
                      find_route_error,
                      self.routing_table.peer_forwarding.keys(),
                      msg,
                );
                return;
            }
        };

        let raw = RawRoutedMessage { target: AccountOrPeerId::PeerId(target), body: msg };
        let msg = self.sign_routed_message(raw);
        self.send_message_to_peer(ctx, msg);
    }

    fn sign_routed_message(&self, msg: RawRoutedMessage) -> RoutedMessage {
        msg.sign(self.peer_id.clone(), &self.config.secret_key)
    }

    fn propose_edge(&self, peer1: PeerId, with_nonce: Option<u64>) -> EdgeInfo {
        let key = Edge::key(self.peer_id.clone(), peer1);

        // When we create a new edge we increase the latest nonce by 2 in case we miss a removal
        // proposal from our partner.
        let nonce = with_nonce.unwrap_or_else(|| self.routing_table.find_nonce(&key) + 2);

        EdgeInfo::new(key.0, key.1, nonce, &EdgeType::Added, &self.config.secret_key)
    }

    // TODO(MarX, #1312): Store ping/pong for testing
    fn handle_ping(&mut self, _ping: Ping) {}

    fn handle_pong(&mut self, _pong: Pong) {}
}

impl Actor for PeerManagerActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Start server if address provided.
        if let Some(server_addr) = self.config.addr {
            // TODO: for now crashes if server didn't start.
            let listener = TcpListener::bind(&server_addr).unwrap();
            info!(target: "info", "Server listening at {}@{}", self.peer_id, server_addr);
            ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(InboundTcpConnect::new));
        }

        // Start peer monitoring.
        self.monitor_peers(ctx);

        // Start active peer stats querying.
        self.monitor_peer_stats(ctx);
    }
}

impl Handler<NetworkRequests> for PeerManagerActor {
    type Result = NetworkResponses;

    fn handle(&mut self, msg: NetworkRequests, ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            NetworkRequests::FetchInfo => {
                let (sent_bytes_per_sec, received_bytes_per_sec) = self.get_total_bytes_per_sec();

                let known_producers =
                    self.routing_table.account_peers.keys().cloned().collect::<Vec<_>>();

                NetworkResponses::Info(NetworkInfo {
                    active_peers: self
                        .active_peers
                        .values()
                        .map(|a| a.full_peer_info.clone())
                        .collect::<Vec<_>>(),
                    num_active_peers: self.num_active_peers(),
                    peer_max_count: self.config.peer_max_count,
                    most_weight_peers: self.most_weight_peers(),
                    sent_bytes_per_sec,
                    received_bytes_per_sec,
                    known_producers,
                })
            }
            NetworkRequests::Block { block } => {
                self.broadcast_message(ctx, SendMessage { message: PeerMessage::Block(block) });
                NetworkResponses::NoResponse
            }
            NetworkRequests::BlockHeaderAnnounce { header, approval } => {
                if let Some(approval) = approval {
                    if let Some(account_id) = self.config.account_id.clone() {
                        self.send_message_to_account(
                            ctx,
                            &approval.target,
                            RoutedMessageBody::BlockApproval(
                                account_id,
                                approval.hash,
                                approval.signature,
                            ),
                        )
                    }
                }
                self.broadcast_message(
                    ctx,
                    SendMessage { message: PeerMessage::BlockHeaderAnnounce(header) },
                );
                NetworkResponses::NoResponse
            }
            NetworkRequests::BlockRequest { hash, peer_id } => {
                if let Some(active_peer) = self.active_peers.get(&peer_id) {
                    active_peer
                        .addr
                        .do_send(SendMessage { message: PeerMessage::BlockRequest(hash) });
                }
                NetworkResponses::NoResponse
            }
            NetworkRequests::BlockHeadersRequest { hashes, peer_id } => {
                if let Some(active_peer) = self.active_peers.get(&peer_id) {
                    active_peer
                        .addr
                        .do_send(SendMessage { message: PeerMessage::BlockHeadersRequest(hashes) });
                }
                NetworkResponses::NoResponse
            }
            NetworkRequests::StateRequest {
                shard_id,
                hash,
                need_header,
                parts_ranges,
                account_id,
            } => {
                self.send_message_to_account(
                    ctx,
                    &account_id,
                    RoutedMessageBody::StateRequest(shard_id, hash, need_header, parts_ranges),
                );
                NetworkResponses::NoResponse
            }
            NetworkRequests::BanPeer { peer_id, ban_reason } => {
                if let Some(peer) = self.active_peers.get(&peer_id) {
                    let _ = peer.addr.do_send(PeerManagerRequest::BanPeer(ban_reason));
                } else {
                    warn!(target: "network", "Try to ban a disconnected peer: {:?}", peer_id);
                    // Call `ban_peer` in peer manager to trigger action that persists information
                    // of ban in disk.
                    self.ban_peer(&peer_id, ban_reason);
                }

                NetworkResponses::NoResponse
            }
            NetworkRequests::AnnounceAccount(announce_account) => {
                self.announce_account(ctx, announce_account);
                NetworkResponses::NoResponse
            }
            NetworkRequests::ChunkPartRequest { account_id, part_request } => {
                self.send_message_to_account(
                    ctx,
                    &account_id,
                    RoutedMessageBody::ChunkPartRequest(part_request),
                );
                NetworkResponses::NoResponse
            }
            NetworkRequests::ChunkOnePartRequest { account_id, one_part_request } => {
                self.send_message_to_account(
                    ctx,
                    &account_id,
                    RoutedMessageBody::ChunkOnePartRequest(one_part_request),
                );
                NetworkResponses::NoResponse
            }
            NetworkRequests::ChunkOnePartResponse { peer_id, header_and_part } => {
                if let Some(active_peer) = self.active_peers.get(&peer_id) {
                    active_peer.addr.do_send(SendMessage {
                        message: PeerMessage::ChunkOnePart(header_and_part),
                    });
                }
                NetworkResponses::NoResponse
            }
            NetworkRequests::ChunkPart { peer_id, part } => {
                if let Some(active_peer) = self.active_peers.get(&peer_id) {
                    active_peer.addr.do_send(SendMessage { message: PeerMessage::ChunkPart(part) });
                }
                NetworkResponses::NoResponse
            }
            NetworkRequests::ChunkOnePartMessage { account_id, header_and_part } => {
                self.send_message_to_account(
                    ctx,
                    &account_id,
                    RoutedMessageBody::ChunkOnePart(header_and_part),
                );
                NetworkResponses::NoResponse
            }
            NetworkRequests::FetchRoutingTable => {
                NetworkResponses::RoutingTableInfo(self.routing_table.info())
            }
            NetworkRequests::Sync(sync_data) => {
                // TODO(MarX): Don't add edges if it is between us and another peer
                //  Send evidence that we are not already connected to that peer
                //  Handle this case properly (maybe we are on the middle of a handshake, so wait
                //  before saying we are not connected).
                // Process edges and add new edges to the routing table. Also broadcast new edges.
                let SyncData { edges, known_accounts } = sync_data;

                let new_edges: Vec<_> = edges
                    .into_iter()
                    .filter(|edge| self.routing_table.process_edge(edge.clone()))
                    .collect();

                let new_accounts = known_accounts
                    .into_iter()
                    .filter(|(account_id, peer_id)| {
                        self.routing_table.add_account(account_id.clone(), peer_id.clone())
                    })
                    .collect();

                let new_data = SyncData { edges: new_edges, known_accounts: new_accounts };

                // Process new accounts.
                if !new_data.is_empty() {
                    self.broadcast_message(
                        ctx,
                        SendMessage { message: PeerMessage::Sync(new_data) },
                    );
                }

                NetworkResponses::NoResponse
            }
        }
    }
}

impl Handler<InboundTcpConnect> for PeerManagerActor {
    type Result = ();

    fn handle(&mut self, msg: InboundTcpConnect, ctx: &mut Self::Context) {
        self.connect_peer(ctx.address(), msg.stream, PeerType::Inbound, None, None);
    }
}

impl Handler<OutboundTcpConnect> for PeerManagerActor {
    type Result = ();

    fn handle(&mut self, msg: OutboundTcpConnect, ctx: &mut Self::Context) {
        if let Some(addr) = msg.peer_info.addr {
            Resolver::from_registry()
                .send(ConnectAddr(addr))
                .into_actor(self)
                .then(move |res, act, ctx| match res {
                    Ok(res) => match res {
                        Ok(stream) => {
                            debug!(target: "network", "Connected to {}", msg.peer_info);
                            let edge_info = act.propose_edge(msg.peer_info.id.clone(), None);

                            act.connect_peer(
                                ctx.address(),
                                stream,
                                PeerType::Outbound,
                                Some(msg.peer_info),
                                Some(edge_info),
                            );
                            actix::fut::ok(())
                        }
                        Err(err) => {
                            info!(target: "network", "Error connecting to {}: {}", addr, err);
                            act.outgoing_peers.remove(&msg.peer_info.id);
                            actix::fut::err(())
                        }
                    },
                    Err(err) => {
                        info!(target: "network", "Error connecting to {}: {}", addr, err);
                        act.outgoing_peers.remove(&msg.peer_info.id);
                        actix::fut::err(())
                    }
                })
                .wait(ctx);
        } else {
            warn!(target: "network", "Trying to connect to peer with no public address: {:?}", msg.peer_info);
        }
    }
}

impl Handler<Consolidate> for PeerManagerActor {
    type Result = ConsolidateResponse;

    fn handle(&mut self, msg: Consolidate, ctx: &mut Self::Context) -> Self::Result {
        // We already connected to this peer.
        if self.active_peers.contains_key(&msg.peer_info.id) {
            trace!(target: "network", "Dropping handshake (Active Peer). {:?} {:?}", self.peer_id, msg.peer_info.id);
            return ConsolidateResponse(false, None);
        }
        // This is incoming connection but we have this peer already in outgoing.
        // This only happens when both of us connect at the same time, break tie using higher peer id.
        if msg.peer_type == PeerType::Inbound && self.outgoing_peers.contains(&msg.peer_info.id) {
            // We pick connection that has lower id.
            if msg.peer_info.id > self.peer_id {
                trace!(target: "network", "Dropping handshake (Tied). {:?} {:?}", self.peer_id, msg.peer_info.id);
                return ConsolidateResponse(false, None);
            }
        }

        let current_nonce = self
            .routing_table
            .find_nonce(&Edge::key(self.peer_id.clone(), msg.peer_info.id.clone()));

        // Check that the received nonce is greater than the current nonce of this connection.
        if current_nonce >= msg.other_edge_info.nonce {
            trace!(target: "network", "Dropping handshake (Invalid nonce). {:?} {:?}", self.peer_id, msg.peer_info.id);
            // If the check fails don't allow this connection.
            return ConsolidateResponse(false, None);
        }

        let require_response = msg.this_edge_info.is_none();

        let edge_info = msg.this_edge_info.clone().unwrap_or_else(|| {
            self.propose_edge(msg.peer_info.id.clone(), Some(msg.other_edge_info.nonce))
        });

        let edge_info_response = if require_response { Some(edge_info.clone()) } else { None };

        // TODO: double check that address is connectable and add account id.
        self.register_peer(
            FullPeerInfo {
                peer_info: msg.peer_info,
                chain_info: msg.chain_info,
                edge_info: msg.other_edge_info,
            },
            edge_info,
            msg.actor,
            ctx,
        );

        ConsolidateResponse(true, edge_info_response)
    }
}

impl Handler<Unregister> for PeerManagerActor {
    type Result = ();

    fn handle(&mut self, msg: Unregister, _ctx: &mut Self::Context) {
        self.unregister_peer(msg.peer_id);
    }
}

impl Handler<Ban> for PeerManagerActor {
    type Result = ();

    fn handle(&mut self, msg: Ban, _ctx: &mut Self::Context) {
        self.ban_peer(&msg.peer_id, msg.ban_reason);
    }
}

impl Handler<PeersRequest> for PeerManagerActor {
    type Result = PeerList;

    fn handle(&mut self, _msg: PeersRequest, _ctx: &mut Self::Context) -> Self::Result {
        PeerList { peers: self.peer_store.healthy_peers(self.config.max_send_peers) }
    }
}

impl Handler<PeersResponse> for PeerManagerActor {
    type Result = ();

    fn handle(&mut self, msg: PeersResponse, _ctx: &mut Self::Context) {
        self.peer_store.add_peers(
            msg.peers.into_iter().filter(|peer_info| peer_info.id != self.peer_id).collect(),
        );
    }
}

/// "Return" true if this message is for this peer and should be sent to the client.
/// Otherwise try to route this message to the final receiver and return false.
impl Handler<RoutedMessage> for PeerManagerActor {
    type Result = bool;

    fn handle(&mut self, msg: RoutedMessage, ctx: &mut Self::Context) -> Self::Result {
        if self.peer_id == msg.target {
            // Handle Ping and Pong message if they are for us without sending to client.
            // i.e. Return false in case of Ping and Pong
            match msg.body {
                RoutedMessageBody::Ping(ping) => self.handle_ping(ping),
                RoutedMessageBody::Pong(pong) => self.handle_pong(pong),
                _ => return true,
            }

            false
        } else {
            // Otherwise route it to its corresponding destination.
            if msg.expect_response() {
                // TODO(MarX, #1368): Handle route back for message that requires response.
            }
            self.send_message_to_peer(ctx, msg);
            false
        }
    }
}

impl Handler<RawRoutedMessage> for PeerManagerActor {
    type Result = ();

    fn handle(&mut self, msg: RawRoutedMessage, ctx: &mut Self::Context) {
        if let AccountOrPeerId::AccountId(target) = msg.target {
            self.send_message_to_account(ctx, &target, msg.body);
        } else {
            let msg = self.sign_routed_message(msg);
            self.send_message_to_peer(ctx, msg);
        }
    }
}
