use substrate_network_libp2p::{ProtocolId, Secret};

/// current version of the protocol
pub(crate) const CURRENT_VERSION: u32 = 1;

#[derive(Clone, Copy)]
pub struct ProtocolConfig {
    // config information goes here
    pub protocol_id: ProtocolId,
    // This is hacky. Ideally we want public key here, but
    // I haven't figured out how to get public key for a node
    // from substrate libp2p
    pub secret: Secret,
}

impl ProtocolConfig {
    pub fn new(protocol_id: ProtocolId, secret: Secret) -> ProtocolConfig {
        ProtocolConfig { protocol_id, secret }
    }

    pub fn new_with_default_id(secret: Secret) -> ProtocolConfig {
        ProtocolConfig { protocol_id: ProtocolId::default(), secret }
    }
}
