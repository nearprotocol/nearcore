use std::cmp;
use std::collections::HashMap;
use std::io;
use std::sync::RwLock;

use borsh::{BorshDeserialize, BorshSerialize};

use rocksdb::{
    BlockBasedOptions, ColumnFamily, ColumnFamilyDescriptor, Direction, IteratorMode,
    MergeOperands, Options, ReadOptions, WriteBatch, DB,
};
use strum_macros::EnumIter;

use crate::trie::merge_refcounted_records;
use near_primitives::version::DbVersion;
use rocksdb::compaction_filter::Decision;
use std::marker::PhantomPinned;

#[derive(Debug, Clone, PartialEq)]
pub struct DBError(rocksdb::Error);

impl std::fmt::Display for DBError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for DBError {}

impl From<rocksdb::Error> for DBError {
    fn from(err: rocksdb::Error) -> Self {
        DBError(err)
    }
}

impl Into<io::Error> for DBError {
    fn into(self) -> io::Error {
        io::Error::new(io::ErrorKind::Other, self)
    }
}

#[derive(PartialEq, Debug, Copy, Clone, EnumIter, BorshDeserialize, BorshSerialize, Hash, Eq)]
pub enum DBCol {
    /// Column to indicate which version of database this is.
    ColDbVersion = 0,
    ColBlockMisc = 1,
    ColBlock = 2,
    ColBlockHeader = 3,
    ColBlockHeight = 4,
    ColState = 5,
    ColChunkExtra = 6,
    ColTransactionResult = 7,
    ColOutgoingReceipts = 8,
    ColIncomingReceipts = 9,
    ColPeers = 10,
    ColEpochInfo = 11,
    ColBlockInfo = 12,
    ColChunks = 13,
    ColPartialChunks = 14,
    /// Blocks for which chunks need to be applied after the state is downloaded for a particular epoch
    ColBlocksToCatchup = 15,
    /// Blocks for which the state is being downloaded
    ColStateDlInfos = 16,
    ColChallengedBlocks = 17,
    ColStateHeaders = 18,
    ColInvalidChunks = 19,
    ColBlockExtra = 20,
    /// Store hash of a block per each height, to detect double signs.
    ColBlockPerHeight = 21,
    ColStateParts = 22,
    ColEpochStart = 23,
    /// Map account_id to announce_account
    ColAccountAnnouncements = 24,
    /// Next block hashes in the sequence of the canonical chain blocks
    ColNextBlockHashes = 25,
    /// `LightClientBlock`s corresponding to the last final block of each completed epoch
    ColEpochLightClientBlocks = 26,
    ColReceiptIdToShardId = 27,
    ColNextBlockWithNewChunk = 28,
    ColLastBlockWithNewChunk = 29,
    /// Map each saved peer on disk with its component id.
    ColPeerComponent = 30,
    /// Map component id with all edges in this component.
    ColComponentEdges = 31,
    /// Biggest nonce used.
    ColLastComponentNonce = 32,
    /// Transactions
    ColTransactions = 33,
    ColChunkPerHeightShard = 34,
    /// Changes to key-values that we have recorded.
    ColStateChanges = 35,
    ColBlockRefCount = 36,
    ColTrieChanges = 37,
    /// Merkle tree of block hashes
    ColBlockMerkleTree = 38,
    ColChunkHashesByHeight = 39,
    /// Block ordinals.
    ColBlockOrdinal = 40,
    /// GC Count for each column
    ColGCCount = 41,
    /// GC helper column to get all Outcome ids by Block Hash
    ColOutcomesByBlockHash = 42,
    ColTransactionRefCount = 43,
    /// Heights of blocks that have been processed
    ColProcessedBlockHeights = 44,
}

// Do not move this line from enum DBCol
pub const NUM_COLS: usize = 45;

impl std::fmt::Display for DBCol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        let desc = match self {
            Self::ColDbVersion => "db version",
            Self::ColBlockMisc => "miscellaneous block data",
            Self::ColBlock => "block data",
            Self::ColBlockHeader => "block header data",
            Self::ColBlockHeight => "block height",
            Self::ColState => "blockchain state",
            Self::ColChunkExtra => "extra information of trunk",
            Self::ColTransactionResult => "transaction results",
            Self::ColOutgoingReceipts => "outgoing receipts",
            Self::ColIncomingReceipts => "incoming receipts",
            Self::ColPeers => "peer information",
            Self::ColEpochInfo => "epoch information",
            Self::ColBlockInfo => "block information",
            Self::ColChunks => "chunks",
            Self::ColPartialChunks => "partial chunks",
            Self::ColBlocksToCatchup => "blocks need to apply chunks",
            Self::ColStateDlInfos => "blocks downloading",
            Self::ColChallengedBlocks => "challenged blocks",
            Self::ColStateHeaders => "state headers",
            Self::ColInvalidChunks => "invalid chunks",
            Self::ColBlockExtra => "extra block information",
            Self::ColBlockPerHeight => "hash of block per height",
            Self::ColStateParts => "state parts",
            Self::ColEpochStart => "epoch start",
            Self::ColAccountAnnouncements => "account announcements",
            Self::ColNextBlockHashes => "next block hash",
            Self::ColEpochLightClientBlocks => "epoch light client block",
            Self::ColReceiptIdToShardId => "receipt id to shard id",
            Self::ColNextBlockWithNewChunk => "next block with new chunk",
            Self::ColLastBlockWithNewChunk => "last block with new chunk",
            Self::ColPeerComponent => "peer components",
            Self::ColComponentEdges => "component edges",
            Self::ColLastComponentNonce => "last component nonce",
            Self::ColTransactions => "transactions",
            Self::ColChunkPerHeightShard => "hash of chunk per height and shard_id",
            Self::ColStateChanges => "key value changes",
            Self::ColBlockRefCount => "refcount per block",
            Self::ColTrieChanges => "trie changes",
            Self::ColBlockMerkleTree => "block merkle tree",
            Self::ColChunkHashesByHeight => "chunk hashes indexed by height_created",
            Self::ColBlockOrdinal => "block ordinal",
            Self::ColGCCount => "gc count",
            Self::ColOutcomesByBlockHash => "outcomes by block hash",
            Self::ColTransactionRefCount => "refcount per transaction",
            Self::ColProcessedBlockHeights => "processed block heights",
        };
        write!(formatter, "{}", desc)
    }
}

// List of columns for which GC should be implemented
lazy_static! {
    pub static ref SHOULD_COL_GC: Vec<bool> = {
        let mut col_gc = vec![true; NUM_COLS];
        col_gc[DBCol::ColDbVersion as usize] = false; // DB version is unrelated to GC
        col_gc[DBCol::ColBlockMisc as usize] = false;
        col_gc[DBCol::ColBlockHeader as usize] = false; // header sync needs headers
        col_gc[DBCol::ColGCCount as usize] = false; // GC count it self isn't GCed
        col_gc[DBCol::ColBlockHeight as usize] = false; // block sync needs it + genesis should be accessible
        col_gc[DBCol::ColPeers as usize] = false; // Peers is unrelated to GC
        col_gc[DBCol::ColBlockMerkleTree as usize] = false;
        col_gc[DBCol::ColAccountAnnouncements as usize] = false;
        col_gc[DBCol::ColEpochLightClientBlocks as usize] = false;
        col_gc[DBCol::ColPeerComponent as usize] = false; // Peer related info doesn't GC
        col_gc[DBCol::ColLastComponentNonce as usize] = false;
        col_gc[DBCol::ColComponentEdges as usize] = false;
        col_gc[DBCol::ColBlockOrdinal as usize] = false;
        col_gc[DBCol::ColEpochInfo as usize] = false; // https://github.com/nearprotocol/nearcore/pull/2952
        col_gc[DBCol::ColEpochStart as usize] = false; // https://github.com/nearprotocol/nearcore/pull/2952
        col_gc
    };
}

// List of columns for which GC may not be executed even in fully operational node
lazy_static! {
    pub static ref SKIP_COL_GC: Vec<bool> = {
        let mut col_gc = vec![false; NUM_COLS];
        // A node may never restarted
        col_gc[DBCol::ColLastBlockWithNewChunk as usize] = true;
        col_gc[DBCol::ColStateHeaders as usize] = true;
        // True until #2515
        col_gc[DBCol::ColStateParts as usize] = true;
        col_gc
    };
}

pub const HEAD_KEY: &[u8; 4] = b"HEAD";
pub const TAIL_KEY: &[u8; 4] = b"TAIL";
pub const CHUNK_TAIL_KEY: &[u8; 10] = b"CHUNK_TAIL";
pub const SYNC_HEAD_KEY: &[u8; 9] = b"SYNC_HEAD";
pub const HEADER_HEAD_KEY: &[u8; 11] = b"HEADER_HEAD";
pub const LATEST_KNOWN_KEY: &[u8; 12] = b"LATEST_KNOWN";
pub const LARGEST_TARGET_HEIGHT_KEY: &[u8; 21] = b"LARGEST_TARGET_HEIGHT";
pub const VERSION_KEY: &[u8; 7] = b"VERSION";

pub struct DBTransaction {
    pub ops: Vec<DBOp>,
}

pub enum DBOp {
    Insert { col: DBCol, key: Vec<u8>, value: Vec<u8> },
    UpdateRefcount { col: DBCol, key: Vec<u8>, value: Vec<u8> },
    Delete { col: DBCol, key: Vec<u8> },
}

impl DBTransaction {
    pub fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, col: DBCol, key: K, value: V) {
        self.ops.push(DBOp::Insert {
            col,
            key: key.as_ref().to_owned(),
            value: value.as_ref().to_owned(),
        });
    }

    pub fn update_refcount<K: AsRef<[u8]>, V: AsRef<[u8]>>(
        &mut self,
        col: DBCol,
        key: K,
        value: V,
    ) {
        self.ops.push(DBOp::UpdateRefcount {
            col,
            key: key.as_ref().to_owned(),
            value: value.as_ref().to_owned(),
        });
    }

    pub fn delete<K: AsRef<[u8]>>(&mut self, col: DBCol, key: K) {
        self.ops.push(DBOp::Delete { col, key: key.as_ref().to_owned() });
    }
}

pub struct RocksDB {
    db: DB,
    cfs: Vec<*const ColumnFamily>,
    _pin: PhantomPinned,
}

// DB was already Send+Sync. cf and read_options are const pointers using only functions in
// this file and safe to share across threads.
unsafe impl Send for RocksDB {}
unsafe impl Sync for RocksDB {}

pub struct TestDB {
    db: RwLock<Vec<HashMap<Vec<u8>, Vec<u8>>>>,
}

pub trait Database: Sync + Send {
    fn transaction(&self) -> DBTransaction {
        DBTransaction { ops: Vec::new() }
    }
    fn get(&self, col: DBCol, key: &[u8]) -> Result<Option<Vec<u8>>, DBError>;
    fn iter<'a>(&'a self, column: DBCol) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a>;
    fn iter_prefix<'a>(
        &'a self,
        col: DBCol,
        key_prefix: &'a [u8],
    ) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a>;
    fn write(&self, batch: DBTransaction) -> Result<(), DBError>;
}

impl Database for RocksDB {
    fn get(&self, col: DBCol, key: &[u8]) -> Result<Option<Vec<u8>>, DBError> {
        let read_options = rocksdb_read_options();
        let result = self.db.get_cf_opt(unsafe { &*self.cfs[col as usize] }, key, &read_options)?;
        Ok(RocksDB::empty_value_filtering(col, result))
    }

    fn iter<'a>(&'a self, col: DBCol) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a> {
        let read_options = rocksdb_read_options();
        unsafe {
            let cf_handle = &*self.cfs[col as usize];
            let iterator = self.db.iterator_cf_opt(cf_handle, read_options, IteratorMode::Start);
            RocksDB::iter_empty_value_filtering(col, iterator)
        }
    }

    fn iter_prefix<'a>(
        &'a self,
        col: DBCol,
        key_prefix: &'a [u8],
    ) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a> {
        // NOTE: There is no Clone implementation for ReadOptions, so we cannot really reuse
        // `self.read_options` here.
        let mut read_options = rocksdb_read_options();
        read_options.set_prefix_same_as_start(true);
        unsafe {
            let cf_handle = &*self.cfs[col as usize];
            // This implementation is copied from RocksDB implementation of `prefix_iterator_cf` since
            // there is no `prefix_iterator_cf_opt` method.
            let iterator = self
                .db
                .iterator_cf_opt(
                    cf_handle,
                    read_options,
                    IteratorMode::From(key_prefix, Direction::Forward),
                )
                .take_while(move |(key, _value)| key.starts_with(key_prefix));
            RocksDB::iter_empty_value_filtering(col, iterator)
        }
    }

    fn write(&self, transaction: DBTransaction) -> Result<(), DBError> {
        let mut batch = WriteBatch::default();
        for op in transaction.ops {
            match op {
                DBOp::Insert { col, key, value } => unsafe {
                    batch.put_cf(&*self.cfs[col as usize], key, value);
                },
                DBOp::UpdateRefcount { col, key, value } => unsafe {
                    batch.merge_cf(&*self.cfs[col as usize], key, value);
                },
                DBOp::Delete { col, key } => unsafe {
                    batch.delete_cf(&*self.cfs[col as usize], key);
                },
            }
        }
        Ok(self.db.write(batch)?)
    }
}

impl Database for TestDB {
    fn get(&self, col: DBCol, key: &[u8]) -> Result<Option<Vec<u8>>, DBError> {
        Ok(self.db.read().unwrap()[col as usize].get(key).cloned())
    }

    fn iter<'a>(&'a self, col: DBCol) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a> {
        let iterator = self.db.read().unwrap()[col as usize]
            .clone()
            .into_iter()
            .map(|(k, v)| (k.into_boxed_slice(), v.into_boxed_slice()));
        Box::new(iterator)
    }

    fn iter_prefix<'a>(
        &'a self,
        col: DBCol,
        key_prefix: &'a [u8],
    ) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a> {
        Box::new(self.iter(col).filter(move |(key, _value)| key.starts_with(key_prefix)))
    }

    fn write(&self, transaction: DBTransaction) -> Result<(), DBError> {
        let mut db = self.db.write().unwrap();
        for op in transaction.ops {
            match op {
                DBOp::Insert { col, key, value } => db[col as usize].insert(key, value),
                DBOp::UpdateRefcount { col, key, value } => {
                    let mut val = db[col as usize].get(&key).cloned().unwrap_or_default();
                    merge_refcounted_records(&mut val, &value).unwrap();
                    if val.len() != 0 {
                        db[col as usize].insert(key, val)
                    } else {
                        db[col as usize].remove(&key)
                    }
                }
                DBOp::Delete { col, key } => db[col as usize].remove(&key),
            };
        }
        Ok(())
    }
}

fn rocksdb_read_options() -> ReadOptions {
    let mut read_options = ReadOptions::default();
    read_options.set_verify_checksums(false);
    read_options
}

/// DB level options
fn rocksdb_options() -> Options {
    let mut opts = Options::default();

    opts.create_missing_column_families(true);
    opts.create_if_missing(true);
    opts.set_use_fsync(false);
    opts.set_max_open_files(512);
    opts.set_keep_log_file_num(1);
    opts.set_bytes_per_sync(1048576);
    opts.set_write_buffer_size(1024 * 1024 * 512 / 2);
    opts.set_max_bytes_for_level_base(1024 * 1024 * 512 / 2);
    opts.increase_parallelism(cmp::max(1, num_cpus::get() as i32 / 2));
    opts.set_max_total_wal_size(1 * 1024 * 1024 * 1024);

    return opts;
}

fn rocksdb_block_based_options() -> BlockBasedOptions {
    let mut block_opts = BlockBasedOptions::default();
    block_opts.set_block_size(1024 * 16);
    let cache_size = 1024 * 1024 * 512 / 3;
    block_opts.set_lru_cache(cache_size);
    block_opts.set_pin_l0_filter_and_index_blocks_in_cache(true);
    block_opts.set_cache_index_and_filter_blocks(true);
    block_opts.set_bloom_filter(10, true);
    block_opts
}

fn rocksdb_column_options(col_index: usize) -> Options {
    let mut opts = Options::default();
    opts.set_level_compaction_dynamic_level_bytes(true);
    opts.set_block_based_table_factory(&rocksdb_block_based_options());
    opts.optimize_level_style_compaction(1024 * 1024 * 128);
    opts.set_target_file_size_base(1024 * 1024 * 64);
    opts.set_compression_per_level(&[]);
    if col_index == DBCol::ColState as usize {
        opts.set_merge_operator("refcount merge", RocksDB::refcount_merge, None);
        opts.set_compaction_filter("empty value filter", RocksDB::empty_value_filter);
    }
    opts
}

impl RocksDB {
    /// Returns version of the database state on disk.
    pub fn get_version<P: AsRef<std::path::Path>>(path: P) -> Result<DbVersion, DBError> {
        let db = RocksDB::new_read_only(path)?;
        db.get(DBCol::ColDbVersion, VERSION_KEY).map(|result| {
            serde_json::from_slice(
                &result
                    .expect("Failed to find version in first column. Database must be corrupted."),
            )
            .expect("Failed to parse version. Database must be corrupted.")
        })
    }

    fn new_read_only<P: AsRef<std::path::Path>>(path: P) -> Result<Self, DBError> {
        let options = Options::default();
        let cf_names: Vec<_> = vec!["col0".to_string()];
        let db = DB::open_cf_for_read_only(&options, path, cf_names.iter(), false)?;
        let cfs =
            cf_names.iter().map(|n| db.cf_handle(n).unwrap() as *const ColumnFamily).collect();
        Ok(Self { db, cfs, _pin: PhantomPinned })
    }

    pub fn new<P: AsRef<std::path::Path>>(path: P) -> Result<Self, DBError> {
        let options = rocksdb_options();
        let cf_names: Vec<_> = (0..NUM_COLS).map(|col| format!("col{}", col)).collect();
        let cf_descriptors = cf_names.iter().enumerate().map(|(col_index, cf_name)| {
            ColumnFamilyDescriptor::new(cf_name, rocksdb_column_options(col_index))
        });
        let db = DB::open_cf_descriptors(&options, path, cf_descriptors)?;
        let cfs =
            cf_names.iter().map(|n| db.cf_handle(n).unwrap() as *const ColumnFamily).collect();
        Ok(Self { db, cfs, _pin: PhantomPinned })
    }
}

impl TestDB {
    pub fn new() -> Self {
        let db: Vec<_> = (0..NUM_COLS).map(|_| HashMap::new()).collect();
        Self { db: RwLock::new(db) }
    }
}

impl RocksDB {
    /// ColState has refcounted values.
    /// Merge adds refcounts, zero refcount becomes empty value.
    /// Empty values get filtered by get methods, and removed by compaction.
    fn refcount_merge(
        _new_key: &[u8],
        existing_val: Option<&[u8]>,
        operands: &mut MergeOperands,
    ) -> Option<Vec<u8>> {
        let mut result = vec![];
        if let Some(val) = existing_val {
            // Error is only possible if decoding refcount fails (=value is between 1 and 3 bytes)
            merge_refcounted_records(&mut result, val).unwrap();
        }
        for val in operands {
            // Error is only possible if decoding refcount fails (=value is between 1 and 3 bytes)
            merge_refcounted_records(&mut result, val).unwrap();
        }
        Some(result)
    }

    fn empty_value_filter(_level: u32, _key: &[u8], value: &[u8]) -> Decision {
        if value.len() == 0 {
            Decision::Remove
        } else {
            Decision::Keep
        }
    }

    fn empty_value_filtering(column: DBCol, value: Option<Vec<u8>>) -> Option<Vec<u8>> {
        if column == DBCol::ColState && Some(vec![]) == value {
            None
        } else {
            value
        }
    }

    fn iter_empty_value_filtering<'a, I>(
        column: DBCol,
        iterator: I,
    ) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a>
    where
        I: Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a,
    {
        if column == DBCol::ColState {
            Box::new(iterator.filter(|(_k, v)| v.len() != 0))
        } else {
            Box::new(iterator)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::db::DBCol::ColState;
    use crate::db::{rocksdb_read_options, DBError, Database, RocksDB};
    use crate::{create_store, DBCol};

    impl RocksDB {
        fn compact(&self, col: DBCol) {
            self.db.compact_range_cf::<&[u8], &[u8]>(
                unsafe { &*self.cfs[col as usize] },
                None,
                None,
            );
        }

        fn get_no_empty_filtering(
            &self,
            col: DBCol,
            key: &[u8],
        ) -> Result<Option<Vec<u8>>, DBError> {
            let read_options = rocksdb_read_options();
            let result =
                self.db.get_cf_opt(unsafe { &*self.cfs[col as usize] }, key, &read_options)?;
            Ok(result)
        }
    }

    #[test]
    fn rocksdb_merge_sanity() {
        let tmp_dir = tempfile::Builder::new().prefix("_test_snapshot_sanity").tempdir().unwrap();
        let store = create_store(tmp_dir.path().to_str().unwrap());
        assert_eq!(store.get(ColState, &[1]).unwrap(), None);
        {
            let mut store_update = store.store_update();
            store_update.update_refcount(ColState, &[1], &[1, 1, 0, 0, 0]);
            store_update.commit().unwrap();
        }
        {
            let mut store_update = store.store_update();
            store_update.update_refcount(ColState, &[1], &[1, 1, 0, 0, 0]);
            store_update.commit().unwrap();
        }
        assert_eq!(store.get(ColState, &[1]).unwrap(), Some(vec![1, 2, 0, 0, 0]));
        {
            let mut store_update = store.store_update();
            store_update.update_refcount(ColState, &[1], &[1, 255, 255, 255, 255]);
            store_update.commit().unwrap();
        }
        assert_eq!(store.get(ColState, &[1]).unwrap(), Some(vec![1, 1, 0, 0, 0]));
        {
            let mut store_update = store.store_update();
            store_update.update_refcount(ColState, &[1], &[1, 255, 255, 255, 255]);
            store_update.commit().unwrap();
        }
        // Refcount goes to 0 -> get() returns None
        assert_eq!(store.get(ColState, &[1]).unwrap(), None);
        let ptr = (&*store.storage) as *const (dyn Database + 'static);
        let rocksdb = unsafe { &*(ptr as *const RocksDB) };
        // Internally there is an empty value
        assert_eq!(rocksdb.get_no_empty_filtering(ColState, &[1]).unwrap(), Some(vec![]));

        rocksdb.compact(ColState);
        rocksdb.compact(ColState);

        // After compaction the empty value disappears
        assert_eq!(rocksdb.get_no_empty_filtering(ColState, &[1]).unwrap(), None);
        assert_eq!(store.get(ColState, &[1]).unwrap(), None);
    }
}
