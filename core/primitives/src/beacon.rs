use super::block_traits::{SignedBlock, SignedHeader};
use super::hash::{hash_struct, CryptoHash};
use super::types::{AuthorityStake, GroupSignature, PartialSignature};
use super::utils::{proto_to_result, proto_to_type};
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::iter::FromIterator;
use std::convert::{TryFrom, TryInto};
use protobuf::{RepeatedField, SingularPtrField};
use near_protos::chain as chain_proto;

const PROTO_ERROR: &str = "Bad Proto";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct BeaconBlockHeader {
    /// Parent hash.
    pub parent_hash: CryptoHash,
    /// Block index.
    pub index: u64,
    /// Authority proposals.
    pub authority_proposal: Vec<AuthorityStake>,
    /// Hash of the shard block.
    pub shard_block_hash: CryptoHash,
}

impl TryFrom<chain_proto::BeaconBlockHeader> for BeaconBlockHeader {
    type Error = String;

    fn try_from(proto: chain_proto::BeaconBlockHeader) -> Result<Self, Self::Error> {
        let parent_hash = proto.parent_hash.into();
        let index = proto.index;
        let shard_block_hash = proto.shard_block_hash.into();
        let authority_proposal: Result<Vec<_>, _> = proto.authority_proposal
            .into_iter()
            .map(TryInto::try_into)
            .collect();
        authority_proposal.map(|proposal| {
            BeaconBlockHeader {
                parent_hash,
                index,
                authority_proposal: proposal,
                shard_block_hash,
            }
        })
    }
}

impl From<BeaconBlockHeader> for chain_proto::BeaconBlockHeader {
    fn from(header: BeaconBlockHeader) -> Self {
        chain_proto::BeaconBlockHeader {
            parent_hash: header.parent_hash.into(),
            index: header.index,
            authority_proposal: RepeatedField::from_iter(
                header.authority_proposal.into_iter().map(std::convert::Into::into)
            ),
            shard_block_hash: header.shard_block_hash.into(),
            unknown_fields: Default::default(),
            cached_size: Default::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SignedBeaconBlockHeader {
    pub body: BeaconBlockHeader,
    pub hash: CryptoHash,
    pub signature: GroupSignature,
}

impl TryFrom<chain_proto::SignedBeaconBlockHeader> for SignedBeaconBlockHeader {
    type Error = String;

    fn try_from(proto: chain_proto::SignedBeaconBlockHeader) -> Result<Self, Self::Error> {
        let hash = proto.hash.into();
        match (proto_to_type(proto.body), proto_to_type(proto.signature)) {
            (Ok(body), Ok(signature)) => {
                Ok(SignedBeaconBlockHeader {
                    body, hash, signature
                })
            }
            _ => Err(PROTO_ERROR.to_string())
        }
    }
}

impl From<SignedBeaconBlockHeader> for chain_proto::SignedBeaconBlockHeader {
    fn from(header: SignedBeaconBlockHeader) -> Self {
        chain_proto::SignedBeaconBlockHeader {
            body: SingularPtrField::some(header.body.into()),
            hash: header.hash.into(),
            signature: SingularPtrField::some(header.signature.into()),
            unknown_fields: Default::default(),
            cached_size: Default::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct BeaconBlock {
    pub header: BeaconBlockHeader,
}

impl TryFrom<chain_proto::BeaconBlock> for BeaconBlock {
    type Error = String;

    fn try_from(proto: chain_proto::BeaconBlock) -> Result<Self, Self::Error> {
        proto_to_result(proto.header)
            .and_then(TryInto::try_into)
            .map(|header| {
                BeaconBlock {
                    header,
                }
            })
    }
}

impl From<BeaconBlock> for chain_proto::BeaconBlock {
    fn from(block: BeaconBlock) -> Self {
        chain_proto::BeaconBlock {
            header: SingularPtrField::some(block.header.into()),
            unknown_fields: Default::default(),
            cached_size: Default::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SignedBeaconBlock {
    pub body: BeaconBlock,
    pub hash: CryptoHash,
    pub signature: GroupSignature,
}

impl TryFrom<chain_proto::SignedBeaconBlock> for SignedBeaconBlock {
    type Error = String;

    fn try_from(proto: chain_proto::SignedBeaconBlock) -> Result<Self, Self::Error> {
        match (proto_to_type(proto.body), proto_to_type(proto.signature)) {
            (Ok(body), Ok(signature)) => {
                Ok(SignedBeaconBlock {
                    body,
                    hash: proto.hash.into(),
                    signature,
                })
            }
            _ => Err(PROTO_ERROR.to_string())
        }
    }
}

impl From<SignedBeaconBlock> for chain_proto::SignedBeaconBlock {
    fn from(block: SignedBeaconBlock) -> Self {
        chain_proto::SignedBeaconBlock {
            body: SingularPtrField::some(block.body.into()),
            hash: block.hash.into(),
            signature: SingularPtrField::some(block.signature.into()),
            unknown_fields: Default::default(),
            cached_size: Default::default(),
        }
    }
}

impl Borrow<CryptoHash> for SignedBeaconBlock {
    fn borrow(&self) -> &CryptoHash {
        &self.hash
    }
}

impl Hash for SignedBeaconBlock {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state)
    }
}

impl PartialEq for SignedBeaconBlock {
    fn eq(&self, other: &SignedBeaconBlock) -> bool {
        self.hash == other.hash
    }
}

impl Eq for SignedBeaconBlock {}

impl SignedHeader for SignedBeaconBlockHeader {
    #[inline]
    fn block_hash(&self) -> CryptoHash {
        self.hash
    }
    #[inline]
    fn index(&self) -> u64 {
        self.body.index
    }
    #[inline]
    fn parent_hash(&self) -> CryptoHash {
        self.body.parent_hash
    }
}

impl SignedBeaconBlock {
    pub fn new(
        index: u64,
        parent_hash: CryptoHash,
        authority_proposal: Vec<AuthorityStake>,
        shard_block_hash: CryptoHash,
    ) -> SignedBeaconBlock {
        let header = BeaconBlockHeader { index, parent_hash, authority_proposal, shard_block_hash };
        let hash = hash_struct(&header);
        SignedBeaconBlock {
            body: BeaconBlock { header },
            hash,
            signature: GroupSignature::default(),
        }
    }

    pub fn genesis(shard_block_hash: CryptoHash) -> SignedBeaconBlock {
        SignedBeaconBlock::new(0, CryptoHash::default(), vec![], shard_block_hash)
    }
}

impl SignedBlock for SignedBeaconBlock {
    type SignedHeader = SignedBeaconBlockHeader;

    fn header(&self) -> Self::SignedHeader {
        SignedBeaconBlockHeader {
            body: self.body.header.clone(),
            hash: self.hash,
            signature: self.signature.clone(),
        }
    }

    #[inline]
    fn index(&self) -> u64 {
        self.body.header.index
    }

    #[inline]
    fn block_hash(&self) -> CryptoHash {
        self.hash
    }

    fn add_signature(&mut self, signature: &PartialSignature, authority_id: usize) {
        self.signature.add_signature(signature, authority_id);
    }

    fn weight(&self) -> u128 {
        // TODO(#279): sum stakes instead of counting them
        self.signature.authority_count() as u128
    }
}
