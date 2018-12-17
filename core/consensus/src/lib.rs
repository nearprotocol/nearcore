//! This library contains tools for consensus that are not dependent on specific implementation of
//! TxFlow or other gossip-based consensus protocol. It also provides simple pass-through consensus
//! that can be used for DevNet.
#[macro_use]
extern crate log;
extern crate rand;
extern crate chrono;
extern crate tokio;
extern crate futures;
extern crate typed_arena;
extern crate primitives;
pub mod adapters;
