use std::sync::Arc;

use crate::storages::beacon::BeaconChainStorage;
use crate::storages::shard::ShardChainStorage;
use crate::storages::total_columns;
use crate::Trie;

/// Creates one beacon storage and one shard storage using in-memory database.
pub fn create_beacon_shard_storages() -> (Arc<BeaconChainStorage>, Arc<ShardChainStorage>) {
    let db = Arc::new(kvdb_memorydb::create(total_columns(1)));
    let beacon = BeaconChainStorage::new(db.clone());
    let shard = ShardChainStorage::new(db.clone(), 0);
    (Arc::new(beacon), Arc::new(shard))
}

/// Creates a Trie using a single shard storage that uses in-memory database.
pub fn create_trie() -> Arc<Trie> {
    let shard_storage = create_beacon_shard_storages().1;
    Arc::new(Trie::new(shard_storage))
}
