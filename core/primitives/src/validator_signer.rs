use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use borsh::BorshSerialize;
use rand::rngs::{OsRng, StdRng};
use rand::SeedableRng;
use serde_derive::{Deserialize, Serialize};

use near_crypto::vrf;
use near_crypto::{InMemorySigner, KeyType, PublicKey, SecretKey, Signature, Signer};

use crate::block::{Approval, BlockHeader, BlockHeaderInnerLite, BlockHeaderInnerRest};
use crate::challenge::ChallengeBody;
use crate::hash::{hash, CryptoHash};
use crate::network::{AnnounceAccount, PeerId};
use crate::sharding::{ChunkHash, ShardChunkHeaderInner};
use crate::types::{AccountId, BlockHeight, EpochId};

/// Validator signer that is used to sign blocks and approvals.
pub trait ValidatorSigner: Sync + Send {
    /// Account id of the given validator.
    fn validator_id(&self) -> &AccountId;

    /// Public key that identifies this validator.
    fn public_key(&self) -> PublicKey;

    /// Sign json (used for info).
    fn sign_json(&self, json: String) -> Signature;

    /// Signs given parts of the header.
    fn sign_block_header_parts(
        &self,
        prev_hash: CryptoHash,
        inner_lite: &BlockHeaderInnerLite,
        inner_rest: &BlockHeaderInnerRest,
    ) -> (CryptoHash, Signature);

    /// Signs given inner of the chunk header.
    fn sign_chunk_header_inner(
        &self,
        chunk_header_inner: &ShardChunkHeaderInner,
    ) -> (ChunkHash, Signature);

    /// Signs approval of given parent hash and reference hash.
    fn sign_approval(
        &self,
        parent_hash: &CryptoHash,
        reference_hash: &Option<CryptoHash>,
        target_height: BlockHeight,
        is_endorsement: bool,
    ) -> Signature;

    /// Signs challenge body.
    fn sign_challenge(&self, challenge_body: &ChallengeBody) -> (CryptoHash, Signature);

    /// Signs account announce.
    fn sign_account_announce(
        &self,
        account_id: &AccountId,
        peer_id: &PeerId,
        epoch_id: &EpochId,
    ) -> Signature;

    /// Used by test infrastructure, only implement if make sense for testing otherwise raise `unimplemented`.
    fn write_to_file(&self, path: &Path);
}

#[derive(Default)]
pub struct EmptyValidatorSigner {
    account_id: AccountId,
}

impl ValidatorSigner for EmptyValidatorSigner {
    fn validator_id(&self) -> &AccountId {
        &self.account_id
    }

    fn public_key(&self) -> PublicKey {
        PublicKey::empty(KeyType::ED25519)
    }

    fn sign_json(&self, _json: String) -> Signature {
        Signature::default()
    }

    fn sign_block_header_parts(
        &self,
        prev_hash: CryptoHash,
        inner_lite: &BlockHeaderInnerLite,
        inner_rest: &BlockHeaderInnerRest,
    ) -> (CryptoHash, Signature) {
        let hash = BlockHeader::compute_hash(prev_hash, inner_lite, inner_rest);
        (hash, Signature::default())
    }

    fn sign_chunk_header_inner(
        &self,
        chunk_header_inner: &ShardChunkHeaderInner,
    ) -> (ChunkHash, Signature) {
        let hash = ChunkHash(hash(&chunk_header_inner.try_to_vec().expect("Failed to serialize")));
        (hash, Signature::default())
    }

    fn sign_approval(
        &self,
        _parent_hash: &CryptoHash,
        _reference_hash: &Option<CryptoHash>,
        _target_height: BlockHeight,
        _is_endorsement: bool,
    ) -> Signature {
        Signature::default()
    }

    fn sign_challenge(&self, challenge_body: &ChallengeBody) -> (CryptoHash, Signature) {
        let hash = hash(&challenge_body.try_to_vec().expect("Failed to serialize"));
        (hash, Signature::default())
    }

    fn sign_account_announce(
        &self,
        _account_id: &String,
        _peer_id: &PeerId,
        _epoch_id: &EpochId,
    ) -> Signature {
        Signature::default()
    }

    fn write_to_file(&self, _path: &Path) {
        unimplemented!()
    }
}

#[derive(Serialize, Deserialize)]
pub struct ValidatorKeyFile {
    pub account_id: String,
    pub public_key: PublicKey,
    pub secret_key: SecretKey,
    pub ristretto_secret_key: vrf::SecretKey,
}

impl ValidatorKeyFile {
    pub fn write_to_file(&self, path: &Path) {
        let mut file = File::create(path).expect("Failed to create / write a key file.");
        let mut perm =
            file.metadata().expect("Failed to retrieve key file metadata.").permissions();
        perm.set_mode(u32::from(libc::S_IWUSR | libc::S_IRUSR));
        file.set_permissions(perm).expect("Failed to set permissions for a key file.");
        let str = serde_json::to_string_pretty(self).expect("Error serializing the key file.");
        if let Err(err) = file.write_all(str.as_bytes()) {
            panic!("Failed to write a key file {}", err);
        }
    }

    pub fn from_file(path: &Path) -> Self {
        let mut file = File::open(path).expect("Could not open key file.");
        let mut content = String::new();
        file.read_to_string(&mut content).expect("Could not read from key file.");
        serde_json::from_str(&content).expect("Failed to deserialize KeyFile")
    }
}

#[derive(Clone)]
pub struct InMemoryRistrettoSigner {
    pub public_key: vrf::PublicKey,
    pub secret_key: vrf::SecretKey,
}

impl InMemoryRistrettoSigner {
    pub fn from_random() -> Self {
        let mut rng = StdRng::from_rng(OsRng::default()).unwrap();
        let secret_key = vrf::SecretKey::random(&mut rng);
        let public_key = secret_key.public_key();
        Self { public_key, secret_key }
    }

    pub fn from_seed(seed: &str) -> Self {
        let secret_key = vrf::SecretKey::from_seed(seed);
        let public_key = secret_key.public_key();
        Self { public_key, secret_key }
    }

    pub fn from_secret_key(secret_key: vrf::SecretKey) -> Self {
        let public_key = secret_key.public_key();
        Self { public_key, secret_key }
    }
}

#[derive(Clone)]
pub struct InMemoryValidatorSigner {
    account_id: AccountId,
    signer: Arc<dyn Signer>,
    ristretto_signer: InMemoryRistrettoSigner,
}

impl InMemoryValidatorSigner {
    pub fn new(account_id: AccountId, signer: Arc<dyn Signer>) -> Self {
        let ristretto_signer = InMemoryRistrettoSigner::from_random();
        Self { account_id, signer, ristretto_signer }
    }

    pub fn from_random(account_id: AccountId, key_type: KeyType) -> Self {
        Self {
            account_id: account_id.clone(),
            signer: Arc::new(InMemorySigner::from_random(account_id, key_type)),
            ristretto_signer: InMemoryRistrettoSigner::from_random(),
        }
    }

    pub fn from_seed(account_id: &str, key_type: KeyType, seed: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            signer: Arc::new(InMemorySigner::from_seed(account_id, key_type, seed)),
            ristretto_signer: InMemoryRistrettoSigner::from_seed(seed),
        }
    }

    pub fn public_key(&self) -> PublicKey {
        self.signer.public_key()
    }

    pub fn from_file(path: &Path) -> Self {
        let validator_key_file = ValidatorKeyFile::from_file(path);
        Self {
            account_id: validator_key_file.account_id.clone(),
            signer: Arc::new(InMemorySigner::from_secret_key(
                validator_key_file.account_id.clone(),
                validator_key_file.secret_key,
            )),
            ristretto_signer: InMemoryRistrettoSigner::from_secret_key(
                validator_key_file.ristretto_secret_key,
            ),
        }
    }
}

impl ValidatorSigner for InMemoryValidatorSigner {
    fn validator_id(&self) -> &AccountId {
        &self.account_id
    }

    fn public_key(&self) -> PublicKey {
        self.signer.public_key()
    }

    fn sign_json(&self, json: String) -> Signature {
        self.signer.sign(json.as_bytes())
    }

    fn sign_block_header_parts(
        &self,
        prev_hash: CryptoHash,
        inner_lite: &BlockHeaderInnerLite,
        inner_rest: &BlockHeaderInnerRest,
    ) -> (CryptoHash, Signature) {
        let hash = BlockHeader::compute_hash(prev_hash, inner_lite, inner_rest);
        (hash, self.signer.sign(hash.as_ref()))
    }

    fn sign_chunk_header_inner(
        &self,
        chunk_header_inner: &ShardChunkHeaderInner,
    ) -> (ChunkHash, Signature) {
        let hash = ChunkHash(hash(&chunk_header_inner.try_to_vec().expect("Failed to serialize")));
        let signature = self.signer.sign(hash.as_ref());
        (hash, signature)
    }

    fn sign_approval(
        &self,
        parent_hash: &CryptoHash,
        reference_hash: &Option<CryptoHash>,
        target_height: BlockHeight,
        is_endorsement: bool,
    ) -> Signature {
        self.signer.sign(&Approval::get_data_for_sig(
            parent_hash,
            reference_hash,
            target_height,
            is_endorsement,
        ))
    }

    fn sign_challenge(&self, challenge_body: &ChallengeBody) -> (CryptoHash, Signature) {
        let hash = hash(&challenge_body.try_to_vec().expect("Failed to serialize"));
        let signature = self.signer.sign(hash.as_ref());
        (hash, signature)
    }

    fn sign_account_announce(
        &self,
        account_id: &AccountId,
        peer_id: &PeerId,
        epoch_id: &EpochId,
    ) -> Signature {
        let hash = AnnounceAccount::build_header_hash(&account_id, &peer_id, epoch_id);
        self.signer.sign(hash.as_ref())
    }

    fn write_to_file(&self, path: &Path) {
        let validator_key_file = ValidatorKeyFile {
            account_id: self.account_id.clone(),
            secret_key: self.signer.secret_key(),
            public_key: self.signer.public_key(),
            ristretto_secret_key: self.ristretto_signer.secret_key,
        };
        validator_key_file.write_to_file(path);
    }
}
