use byteorder::{LittleEndian, WriteBytesExt};
use parking_lot::RwLock;
use primitives::hash::CryptoHash;
use primitives::traits::{Block, Decode, Encode, Header};
use primitives::types::BlockId;
use std::collections::HashMap;
use std::sync::Arc;
use storage::Storage;

const BLOCKCHAIN_BEST_BLOCK: &[u8] = b"best";

/// Configure the chain.
#[derive(Clone)]
pub struct ChainConfig {
    /// Column in storage for extra information.
    pub extra_col: u32,
    /// Column in storage for blocks.
    pub block_col: u32,
    /// Column in storage for headers.
    pub header_col: u32,
    /// Column in storage for index.
    pub index_col: u32,
}

/// General BlockChain container.
pub struct BlockChain<B: Block> {
    /// Chain configuration.
    chain_config: ChainConfig,
    /// Storage backend.
    storage: Arc<Storage>,
    /// Genesis hash
    pub genesis_hash: CryptoHash,
    /// Tip of the known chain.
    best_block: RwLock<B>,
    /// Headers indexed by hash
    headers: RwLock<HashMap<Vec<u8>, B::Header>>,
    /// Blocks indexed by hash
    blocks: RwLock<HashMap<Vec<u8>, B>>,
    /// Maps block index to hash.
    index_to_hash: RwLock<HashMap<Vec<u8>, CryptoHash>>,
    // TODO: state?
}

fn index_to_bytes(index: u64) -> Vec<u8> {
    let mut bytes = vec![];
    bytes
        .write_u64::<LittleEndian>(index)
        .expect("writing to bytes failed");
    bytes
}

fn write_with_cache<T: Clone + Encode>(
    storage: &Arc<Storage>,
    col: u32,
    cache: &RwLock<HashMap<Vec<u8>, T>>,
    key: &[u8],
    value: &T,
) {
    let data = Encode::encode(value).expect("Error serializing data");
    cache.write().insert(key.to_vec(), value.clone());
    storage.set(col, key, &data);
}

fn read_with_cache<T: Clone + Decode>(
    storage: &Arc<Storage>,
    col: u32,
    cache: &RwLock<HashMap<Vec<u8>, T>>,
    key: &[u8],
) -> Option<T> {
    {
        let read = cache.read();
        if let Some(v) = read.get(key) {
            return Some(v.clone());
        }
    }

    // TODO: use columns here.
    Decode::decode(&storage.get(col, key)?).map(|value: T| {
        let mut write = cache.write();
        write.insert(key.to_vec(), value.clone());
        value
    })
}

impl<B: Block> BlockChain<B> {
    pub fn new(chain_config: ChainConfig, genesis: B, storage: Arc<Storage>) -> Self {
        let genesis_hash = genesis.hash();
        let bc = BlockChain {
            chain_config,
            storage,
            genesis_hash,
            best_block: RwLock::new(genesis.clone()),
            headers: RwLock::new(HashMap::new()),
            blocks: RwLock::new(HashMap::new()),
            index_to_hash: RwLock::new(HashMap::new()),
        };

        // Load best block hash from storage.
        let best_block_hash = match bc
            .storage
            .get(bc.chain_config.extra_col, BLOCKCHAIN_BEST_BLOCK)
        {
            Some(best_hash) => CryptoHash::new(&best_hash),
            _ => {
                // Insert genesis block into cache.
                bc.insert_block(genesis);
                genesis_hash
            }
        };

        {
            let mut best_block = bc.best_block.write();
            *best_block = bc
                .get_block(&BlockId::Hash(best_block_hash))
                .expect("Not found best block in the chain");
        }

        // Load best block into cache.
        bc
    }

    pub fn best_block(&self) -> B {
        self.best_block.read().clone()
    }

    /// Check if block already is known.
    pub fn is_known(&self, hash: &CryptoHash) -> bool {
        self.headers.read().contains_key(hash.as_ref()) || self
            .storage
            .exists(self.chain_config.header_col, hash.as_ref())
    }

    /// Inserts a verified block.
    /// Returns true if block is disconnected.
    pub fn insert_block(&self, block: B) -> bool {
        let block_hash = block.hash();
        if self.is_known(&block_hash) {
            return false;
        }

        // Store block in db.
        write_with_cache(
            &self.storage,
            self.chain_config.block_col,
            &self.blocks,
            block_hash.as_ref(),
            &block,
        );
        write_with_cache(
            &self.storage,
            self.chain_config.header_col,
            &self.headers,
            block_hash.as_ref(),
            &block.header(),
        );
        write_with_cache(
            &self.storage,
            self.chain_config.index_col,
            &self.index_to_hash,
            &index_to_bytes(block.header().index()),
            &block_hash,
        );

        let maybe_parent = self.get_header(&BlockId::Hash(block.header().parent_hash()));
        if let Some(_parent_details) = maybe_parent {
            // TODO: rewind parents if they were not processed somehow?
            if block.header().index() > self.best_block.read().header().index() {
                let mut best_block = self.best_block.write();
                *best_block = block;
                self.storage.set(
                    self.chain_config.extra_col,
                    BLOCKCHAIN_BEST_BLOCK,
                    block_hash.as_ref(),
                );
            }
            false
        } else {
            true
        }
    }

    fn get_block_hash_by_index(&self, index: u64) -> Option<CryptoHash> {
        read_with_cache(
            &self.storage,
            self.chain_config.index_col,
            &self.index_to_hash,
            &index_to_bytes(index),
        )
    }

    fn get_block_by_hash(&self, block_hash: &CryptoHash) -> Option<B> {
        read_with_cache(
            &self.storage,
            self.chain_config.block_col,
            &self.blocks,
            block_hash.as_ref(),
        )
    }

    pub fn get_block(&self, id: &BlockId) -> Option<B> {
        match id {
            BlockId::Number(num) => self.get_block_by_hash(&self.get_block_hash_by_index(*num)?),
            BlockId::Hash(hash) => self.get_block_by_hash(hash),
        }
    }

    fn get_block_header_by_hash(&self, block_hash: &CryptoHash) -> Option<B::Header> {
        read_with_cache(
            &self.storage,
            self.chain_config.header_col,
            &self.headers,
            block_hash.as_ref(),
        )
    }

    pub fn get_header(&self, id: &BlockId) -> Option<B::Header> {
        match id {
            BlockId::Number(num) => {
                self.get_block_header_by_hash(&self.get_block_hash_by_index(*num)?)
            }
            BlockId::Hash(hash) => self.get_block_header_by_hash(hash),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use storage::MemoryStorage;
    use types::BeaconBlock;

    #[test]
    fn test_genesis() {
        let storage = Arc::new(MemoryStorage::default());
        let chain_config = ChainConfig {
            extra_col: 0,
            header_col: 1,
            block_col: 2,
            index_col: 3,
        };
        let genesis = BeaconBlock::new(0, CryptoHash::default(), vec![]);
        let bc = Blockchain::new(chain_config, genesis.clone(), storage);
        assert_eq!(
            bc.get_block(&BlockId::Hash(genesis.hash())).unwrap(),
            genesis
        );
        assert_eq!(bc.get_block(&BlockId::Number(0)).unwrap(), genesis);
    }

    #[test]
    fn test_restart_chain() {
        let storage = Arc::new(MemoryStorage::default());
        let chain_config = ChainConfig {
            extra_col: 0,
            header_col: 1,
            block_col: 2,
            index_col: 3,
        };
        let genesis = BeaconBlock::new(0, CryptoHash::default(),  vec![]);
        let bc = BlockChain::new(chain_config.clone(), genesis.clone(), storage.clone());
        let block1 = BeaconBlock::new(1, genesis.hash(), vec![]);
        assert_eq!(bc.insert_block(block1.clone()), false);
        assert_eq!(bc.best_block().hash(), block1.hash());
        assert_eq!(bc.best_block().header.index, 1);
        // Create new BlockChain that reads from the same storage.
        let other_bc = BlockChain::new(chain_config, genesis.clone(), storage.clone());
        assert_eq!(other_bc.best_block().hash(), block1.hash());
        assert_eq!(other_bc.best_block().header.index, 1);
        assert_eq!(
            other_bc.get_block(&BlockId::Hash(block1.hash())).unwrap(),
            block1
        );
    }
}
