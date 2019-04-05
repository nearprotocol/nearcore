use std::fs;
use std::path::Path;
use std::process;

use crate::crypto::aggregate_signature::{BlsPublicKey, BlsSecretKey};
use crate::crypto::signature::{
    bs58_pub_key_format, bs58_secret_key_format, bs58_serializer, get_key_pair, sign, PublicKey,
    SecretKey, Signature,
};
use crate::types::{AccountId, PartialSignature};
use cached::{Cached, SizedCache};
use rand::distributions::Alphanumeric;
use rand::rngs::OsRng;
use rand::Rng;
use std::sync::Mutex;

/// Trait to abstract the way transaction signing happens.
pub trait TransactionSigner: Sync + Send {
    fn public_key(&self) -> PublicKey;
    fn sign(&self, hash: &[u8]) -> Signature;
}

/// Trait to abstract the way signing happens for block production.
/// Can be used to not keep private key in the given binary via cross-process communication.
pub trait BlockSigner: Sync + Send + TransactionSigner {
    fn bls_public_key(&self) -> BlsPublicKey;
    fn bls_sign(&self, hash: &[u8]) -> PartialSignature;
    fn bls_sign_cached(&self, hash: &[u8]) -> PartialSignature;
    fn account_id(&self) -> AccountId;
}

#[derive(Serialize, Deserialize)]
pub struct KeyFile {
    #[serde(with = "bs58_pub_key_format")]
    pub public_key: PublicKey,
    #[serde(with = "bs58_secret_key_format")]
    pub secret_key: SecretKey,
}

pub fn write_key_file(
    key_store_path: &Path,
    public_key: PublicKey,
    secret_key: SecretKey,
) -> String {
    if !key_store_path.exists() {
        fs::create_dir_all(key_store_path).unwrap();
    }

    let key_file = KeyFile { public_key, secret_key };
    let key_file_path = key_store_path.join(Path::new(&public_key.to_string()));
    let serialized = serde_json::to_string(&key_file).unwrap();
    fs::write(key_file_path, serialized).unwrap();
    public_key.to_string()
}

pub fn get_key_file(key_store_path: &Path, public_key: Option<String>) -> KeyFile {
    if !key_store_path.exists() {
        println!("Key store path does not exist: {:?}", &key_store_path);
        process::exit(3);
    }

    let mut key_files = fs::read_dir(key_store_path).unwrap();
    let key_file = key_files.next();
    let key_file_string = if key_files.count() != 0 {
        if let Some(p) = public_key {
            let key_file_path = key_store_path.join(Path::new(&p));
            fs::read_to_string(key_file_path).unwrap()
        } else {
            println!(
                "Public key must be specified when there is more than one \
                 file in the keystore"
            );
            process::exit(4);
        }
    } else {
        fs::read_to_string(key_file.unwrap().unwrap().path()).unwrap()
    };

    serde_json::from_str(&key_file_string).unwrap()
}

#[derive(Serialize, Deserialize)]
pub struct BlockProducerKeyFile {
    #[serde(with = "bs58_serializer")]
    pub public_key: PublicKey,
    #[serde(with = "bs58_serializer")]
    pub secret_key: SecretKey,
    #[serde(with = "bs58_serializer")]
    pub bls_public_key: BlsPublicKey,
    #[serde(with = "bs58_serializer")]
    pub bls_secret_key: BlsSecretKey,
}

pub fn write_block_producer_key_file(
    key_store_path: &Path,
    public_key: PublicKey,
    secret_key: SecretKey,
    bls_public_key: BlsPublicKey,
    bls_secret_key: BlsSecretKey,
) -> String {
    if !key_store_path.exists() {
        fs::create_dir_all(key_store_path).unwrap();
    }

    let key_file = BlockProducerKeyFile { public_key, secret_key, bls_public_key, bls_secret_key };
    let key_file_path = key_store_path.join(Path::new(&key_file.public_key.to_string()));
    let serialized = serde_json::to_string(&key_file).unwrap();
    fs::write(key_file_path, serialized).unwrap();
    key_file.public_key.to_string()
}

pub fn get_block_producer_key_file(
    key_store_path: &Path,
    public_key: Option<String>,
) -> BlockProducerKeyFile {
    if !key_store_path.exists() {
        println!("Key store path does not exist: {:?}", &key_store_path);
        process::exit(3);
    }

    let mut key_files = fs::read_dir(key_store_path).unwrap();
    let key_file = key_files.next();
    let key_files_count = key_files.count();
    if key_files_count == 0 && key_file.is_none() {
        panic!("No key file found in {:?}. Run `cargo run --package keystore -- keygen --test-seed alice.near` to set up testing keys.", key_store_path);
    }
    let key_file_string = if key_files_count > 0 {
        if let Some(p) = public_key {
            let key_file_path = key_store_path.join(Path::new(&p));
            match fs::read_to_string(key_file_path.clone()) {
                Ok(content) => content,
                Err(err) => {
                    panic!("Failed to read key file {:?} with error: {}", key_file_path, err);
                }
            }
        } else {
            println!(
                "Public key must be specified when there is more than one \
                 file in the keystore"
            );
            process::exit(4);
        }
    } else {
        let path = key_file.unwrap().unwrap().path();
        match fs::read_to_string(path.clone()) {
            Ok(content) => content,
            Err(err) => {
                panic!("Failed to read key file {:?} with error: {}", path, err);
            }
        }
    };

    serde_json::from_str(&key_file_string).unwrap()
}

pub fn get_or_create_key_file(
    key_store_path: &Path,
    public_key: Option<String>,
) -> BlockProducerKeyFile {
    if !key_store_path.exists() {
        let (public_key, secret_key) = get_key_pair();
        let bls_secret_key = BlsSecretKey::generate();
        let bls_public_key = bls_secret_key.get_public_key();
        let new_public_key = write_block_producer_key_file(
            key_store_path,
            public_key,
            secret_key,
            bls_public_key,
            bls_secret_key,
        );
        get_block_producer_key_file(key_store_path, Some(new_public_key))
    } else {
        get_block_producer_key_file(key_store_path, public_key)
    }
}

pub struct InMemorySigner {
    pub account_id: AccountId,
    pub public_key: PublicKey,
    pub secret_key: SecretKey,
    pub bls_public_key: BlsPublicKey,
    pub bls_secret_key: BlsSecretKey,
    /// Cache for BLS signatures to make sure we do not recompute them.
    pub bls_cache: Mutex<SizedCache<Vec<u8>, PartialSignature>>,
}

const BLS_CACHE_SIZE: usize = 100_000;

impl InMemorySigner {
    pub fn from_key_file(
        account_id: AccountId,
        key_store_path: &Path,
        public_key: Option<String>,
    ) -> Self {
        let key_file = get_or_create_key_file(key_store_path, public_key);
        InMemorySigner {
            account_id,
            public_key: key_file.public_key,
            secret_key: key_file.secret_key,
            bls_public_key: key_file.bls_public_key,
            bls_secret_key: key_file.bls_secret_key,
            bls_cache: Mutex::new(SizedCache::with_size(BLS_CACHE_SIZE)),
        }
    }

    /// Initialize `InMemorySigner` with a random ED25519 and BLS keys, and random account id. Used
    /// for testing only.
    pub fn from_random() -> Self {
        let mut rng = OsRng::new().expect("Unable to generate random numbers");
        let account_id: String =
            rng.sample_iter(&Alphanumeric).filter(char::is_ascii_alphabetic).take(10).collect();
        let (public_key, secret_key) = get_key_pair();
        let bls_secret_key = BlsSecretKey::generate();
        let bls_public_key = bls_secret_key.get_public_key();
        Self {
            account_id,
            public_key,
            secret_key,
            bls_public_key,
            bls_secret_key,
            bls_cache: Mutex::new(SizedCache::with_size(BLS_CACHE_SIZE)),
        }
    }
}

impl TransactionSigner for InMemorySigner {
    #[inline]
    fn public_key(&self) -> PublicKey {
        self.public_key
    }

    fn sign(&self, data: &[u8]) -> Signature {
        sign(data, &self.secret_key)
    }
}

impl BlockSigner for InMemorySigner {
    #[inline]
    fn bls_public_key(&self) -> BlsPublicKey {
        self.bls_public_key.clone()
    }

    fn bls_sign(&self, hash: &[u8]) -> PartialSignature {
        self.bls_secret_key.sign(hash)
    }

    fn bls_sign_cached(&self, data: &[u8]) -> PartialSignature {
        let mut guard = self.bls_cache.lock().expect("Cache lock was poisoned");
        let key = data.to_vec();
        if let Some(res) = guard.cache_get(&key) {
            return res.clone();
        }

        let res = self.bls_sign(data);
        guard.cache_set(key, res.clone());
        res
    }

    #[inline]
    fn account_id(&self) -> AccountId {
        self.account_id.clone()
    }
}
