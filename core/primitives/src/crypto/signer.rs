use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process;

use rand::distributions::Alphanumeric;
use rand::rngs::OsRng;
use rand::Rng;

use crate::crypto::aggregate_signature::{BlsPublicKey, BlsSecretKey};
use crate::crypto::signature::{
    bs64_pub_key_format, bs64_secret_key_format, bs64_serializer, get_key_pair, sign, PublicKey,
    SecretKey, Signature,
};
use crate::types::{AccountId, PartialSignature};

/// Trait to abstract the signer account.
pub trait AccountSigner: Sync + Send {
    fn account_id(&self) -> AccountId;
}

/// Trait to abstract the way transaction signing with ed25519.
/// Can be used to not keep private key in the given binary via cross-process communication.
pub trait EDSigner: Sync + Send {
    fn public_key(&self) -> PublicKey;
    fn sign(&self, data: &[u8]) -> Signature;
}

/// Trait to abstract the way signing with bls.
/// Can be used to not keep private key in the given binary via cross-process communication.
pub trait BLSSigner: Sync + Send {
    fn bls_public_key(&self) -> BlsPublicKey;
    fn bls_sign(&self, data: &[u8]) -> PartialSignature;
}

#[derive(Serialize, Deserialize)]
pub struct KeyFile {
    #[serde(with = "bs64_pub_key_format")]
    pub public_key: PublicKey,
    #[serde(with = "bs64_secret_key_format")]
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
    #[serde(with = "bs64_serializer")]
    pub public_key: PublicKey,
    #[serde(with = "bs64_serializer")]
    pub secret_key: SecretKey,
    //    #[serde(with = "bs64_serializer")]
    //    pub bls_public_key: BlsPublicKey,
    //    #[serde(with = "bs64_serializer")]
    //    pub bls_secret_key: BlsSecretKey,
}

pub fn write_block_producer_key_file(
    key_store_path: &Path,
    public_key: PublicKey,
    secret_key: SecretKey,
    //    bls_public_key: BlsPublicKey,
    //    bls_secret_key: BlsSecretKey,
) -> String {
    if !key_store_path.exists() {
        fs::create_dir_all(key_store_path).unwrap();
    }

    let key_file = BlockProducerKeyFile {
        public_key,
        secret_key,
        // bls_public_key, bls_secret_key
    };
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
        //        let bls_secret_key = BlsSecretKey::generate();
        //        let bls_public_key = bls_secret_key.get_public_key();
        let new_public_key = write_block_producer_key_file(
            key_store_path,
            public_key,
            secret_key,
            //            bls_public_key,
            //            bls_secret_key,
        );
        get_block_producer_key_file(key_store_path, Some(new_public_key))
    } else {
        get_block_producer_key_file(key_store_path, public_key)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InMemorySigner {
    pub account_id: AccountId,
    #[serde(with = "bs64_serializer")]
    pub public_key: PublicKey,
    #[serde(with = "bs64_serializer")]
    pub secret_key: SecretKey,
}

impl InMemorySigner {
    pub fn new(account_id: String) -> Self {
        let (public_key, secret_key) = get_key_pair();
        Self { account_id, public_key, secret_key }
    }

    /// Read key file into signer.
    pub fn from_file(path: &Path) -> Self {
        let mut file = File::open(path).expect("Could not open key file.");
        let mut content = String::new();
        file.read_to_string(&mut content).expect("Could not read from key file.");
        InMemorySigner::from(content.as_str())
    }

    /// Save signer into key file.
    pub fn write_to_file(&self, path: &Path) {
        let mut file = File::create(path).expect("Failed to create / write a key file.");
        let str = serde_json::to_string_pretty(self).expect("Error serializing the key file.");
        if let Err(err) = file.write_all(str.as_bytes()) {
            panic!("Failed to write a key file {}", err);
        }
    }

    /// Initialize `InMemorySigner` with a random ED25519 and BLS keys, and random account id. Used
    /// for testing only.
    pub fn from_random() -> Self {
        let mut rng = OsRng::new().expect("Unable to generate random numbers");
        let account_id: String =
            rng.sample_iter(&Alphanumeric).filter(char::is_ascii_alphabetic).take(10).collect();
        let (public_key, secret_key) = get_key_pair();
        Self { account_id, public_key, secret_key }
    }
}

impl From<&str> for InMemorySigner {
    fn from(key_file: &str) -> Self {
        serde_json::from_str(key_file).expect("Failed to deserialize the key file.")
    }
}

impl AccountSigner for InMemorySigner {
    #[inline]
    fn account_id(&self) -> AccountId {
        self.account_id.clone()
    }
}

impl EDSigner for InMemorySigner {
    #[inline]
    fn public_key(&self) -> PublicKey {
        self.public_key
    }

    fn sign(&self, data: &[u8]) -> Signature {
        sign(data, &self.secret_key)
    }
}

//impl BLSSigner for InMemorySigner {
//    #[inline]
//    fn bls_public_key(&self) -> BlsPublicKey {
//        self.bls_public_key.clone()
//    }
//
//    fn bls_sign(&self, data: &[u8]) -> PartialSignature {
//        self.bls_secret_key.sign(data)
//    }
//}
