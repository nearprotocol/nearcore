use crate::types::Balance;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Data structure for semver version and github tag or commit.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Version {
    pub version: String,
    pub build: String,
}

/// Database version.
pub type DbVersion = u32;

/// Current version of the database.
pub const DB_VERSION: DbVersion = 14;

/// Protocol version type.
pub type ProtocolVersion = u32;

/// Oldest supported version by this client.
pub const OLDEST_BACKWARD_COMPATIBLE_PROTOCOL_VERSION: ProtocolVersion = 34;

/// Minimum gas price proposed in NEP 92 and the associated protocol version
pub const MIN_GAS_PRICE_NEP_92: Balance = 1_000_000_000;
pub const MIN_PROTOCOL_VERSION_NEP_92: ProtocolVersion = 31;

/// Minimum gas price proposed in NEP 92 (fixed) and the associated protocol version
pub const MIN_GAS_PRICE_NEP_92_FIX: Balance = 100_000_000;
pub const MIN_PROTOCOL_VERSION_NEP_92_FIX: ProtocolVersion = 32;

pub const CORRECT_RANDOM_VALUE_PROTOCOL_VERSION: ProtocolVersion = 33;

/// See [NEP 71](https://github.com/nearprotocol/NEPs/pull/71)
pub const IMPLICIT_ACCOUNT_CREATION_PROTOCOL_VERSION: ProtocolVersion = 35;

/// The protocol version that enables reward on mainnet.
pub const ENABLE_INFLATION_PROTOCOL_VERSION: ProtocolVersion = 36;

/// Fix upgrade to use the latest voted protocol version instead of the current epoch protocol
/// version when there is no new change in protocol version.
pub const UPGRADABILITY_FIX_PROTOCOL_VERSION: ProtocolVersion = 37;

/// Updates the way receipt ID, data ID and random seeds are constructed.
pub const CREATE_HASH_PROTOCOL_VERSION: ProtocolVersion = 38;

pub const SHARD_CHUNK_HEADER_UPGRADE_VERSION: ProtocolVersion = 40;
pub const CHUNK_FORWARD_UPGRADE_VERSION: ProtocolVersion = 41;

pub struct ProtocolVersionRange {
    lower: ProtocolVersion,
    upper: Option<ProtocolVersion>,
}

impl ProtocolVersionRange {
    pub fn new(lower: ProtocolVersion, upper: Option<ProtocolVersion>) -> Self {
        Self { lower, upper }
    }

    pub fn contains(&self, version: ProtocolVersion) -> bool {
        self.lower <= version && self.upper.map_or(true, |upper| version < upper)
    }
}

/// New Protocol features should go here. Features are guarded by their corresponding feature flag.
/// For example, if we have `ProtocolFeature::EVM` and a corresponding feature flag `evm`, it will look
/// like
/// ```
/// #[cfg(feature = "evm")]
/// EVM
/// ```
#[derive(Hash, PartialEq, Eq)]
pub enum ProtocolFeature {}

/// Current latest stable version of the protocol.
#[cfg(not(feature = "nightly_protocol"))]
pub const PROTOCOL_VERSION: ProtocolVersion = 40;

/// Current latest nightly version of the protocol.
#[cfg(feature = "nightly_protocol")]
pub const PROTOCOL_VERSION: ProtocolVersion = 41;

/// Map of feature to the minimal protocol version that introduces the feature. We can determine
/// whether to apply the new feature by comparing the current protocol version of the network to
/// `PROTOCOL_FEATURES_TO_VERSION_MAPPING[feature]`.
#[cfg(not(feature = "nightly_protocol"))]
lazy_static! {
    static ref PROTOCOL_FEATURES_TO_VERSION_MAPPING: HashMap<ProtocolFeature, ProtocolVersion> = vec![
        /* add mapping here */
    ].into_iter().collect();
}

#[cfg(feature = "nightly_protocol")]
lazy_static! {
    static ref PROTOCOL_FEATURES_TO_VERSION_MAPPING: HashMap<ProtocolFeature, ProtocolVersion> = vec![
        /* add mapping here */
    ].into_iter().collect();
}
