use chrono::Utc;
use near_primitives::hash::CryptoHash;
use near_primitives::network::PeerId;
use std::collections::btree_map;
use std::collections::{BTreeMap, BTreeSet, HashMap};
/// Cache to store route back messages.
///
/// The interface of the cache is similar to a regular HashMap:
/// elements can be inserted, fetched and removed.
///
/// Motivation behind the following (complex) design:
///
/// Since this cache is populated with messages that should be routed
/// back it is important that elements are not removed unless there is
/// some confidence that the response is not going to arrive.
///
/// A naive cache can be easily abused, since a malicious actor can send
/// void messages that should be routed back, letting users replacing useful
/// entries on the cache for fake ones, and producing most of the messages
/// that require route back being dropped.
///
/// Solution:
///
/// The cache will accept new elements until it is full. If it receives a new
/// element but it implies going over the capacity, the message to remove is
/// selected as following:
///
/// 1. For every message store the time it arrives.
/// 2. For every peer store how many message should be routed to it.
///
/// First are removed messages that have been in the cache more time than
/// EVICTED_TIMEOUT. If no message was removed, it is removed the oldest
/// message from the peer with more messages in the cache.
///
/// Rationale:
///
/// - Old entries in the cache will be eventually removed (no memory leak).
/// - If the cache is not at full capacity, all new records will be stored.
/// - If a peer try to abuse the system, it will be able to allocate at most
///     $capacity / number_of_active_connections$ entries.
pub struct RouteBackCache {
    /// Maximum number of records allowed in the cache.
    capacity: i64,
    /// Maximum time allowed before removing a record from the cache.
    evict_timeout: i64,
    /// Main map from message hash to time where it was created + target peer
    /// Size: O(capacity)
    main: HashMap<CryptoHash, (i64, PeerId)>,
    /// Number of records allocated by each PeerId.
    /// The size is stored with negative sign, to order in PeerId in decreasing order.
    /// Size: O(number of active connections)
    size_per_target: BTreeSet<(i64, PeerId)>,
    /// List of all hashes associated with each PeerId. Hashes within each PeerId
    /// are sorted by the time they arrived from older to newer.
    /// Size: O(capacity)
    record_per_target: BTreeMap<PeerId, BTreeSet<(i64, CryptoHash)>>,
}

impl RouteBackCache {
    fn is_full(&self) -> bool {
        self.capacity == self.main.len() as i64
    }

    fn remove_frequent(&mut self) {
        let (mut size, target) = self.size_per_target.iter().next().cloned().unwrap();

        if let btree_map::Entry::Occupied(mut entry) = self.record_per_target.entry(target.clone())
        {
            {
                let records = entry.get_mut();
                let first = records.iter().next().cloned().unwrap();
                records.remove(&first);
                self.main.remove(&first.1);
            }

            if entry.get().is_empty() {
                entry.remove();
            }
        } else {
            unreachable!();
        }

        self.size_per_target.remove(&(size, target.clone()));
        // Since size has negative sign, adding 1, is equivalent to subtracting 1 from the size.
        size += 1;

        if size != 0 {
            self.size_per_target.insert((size, target));
        }
    }

    fn remove_evicted(&mut self) {
        if self.is_full() {
            let now = Utc::now().timestamp_millis();
            let remove_until = now.saturating_sub(self.evict_timeout);

            let mut new_size_per_target = BTreeSet::new();

            let mut remove_empty = vec![];
            let mut remove_hash = vec![];

            for (key, value) in self.record_per_target.iter_mut() {
                let keep = value.split_off(&(remove_until, CryptoHash::default()));

                for evicted in value.iter() {
                    remove_hash.push(evicted.1.clone())
                }

                *value = keep;

                if !value.is_empty() {
                    new_size_per_target.insert((-(value.len() as i64), key.clone()));
                } else {
                    remove_empty.push(key.clone());
                }
            }

            for key in remove_empty {
                self.record_per_target.remove(&key);
            }

            for h in remove_hash {
                self.main.remove(&h);
            }

            std::mem::swap(&mut new_size_per_target, &mut self.size_per_target);
        }
    }

    pub fn new(capacity: u64, evict_timeout: u64) -> Self {
        assert!(capacity > 0);

        Self {
            capacity: capacity as i64,
            evict_timeout: evict_timeout as i64,
            main: HashMap::new(),
            size_per_target: BTreeSet::new(),
            record_per_target: BTreeMap::new(),
        }
    }

    pub fn get(&self, hash: &CryptoHash) -> Option<&PeerId> {
        self.main.get(&hash).map(|(_, target)| target)
    }

    pub fn remove(&mut self, hash: &CryptoHash) -> Option<PeerId> {
        self.remove_evicted();

        if let Some((time, target)) = self.main.remove(hash) {
            // Number of elements associated with this target
            let mut size = self.record_per_target.get(&target).map(|x| x.len() as i64).unwrap();

            // Remove from `size_per_target` since value is going to be updated
            self.size_per_target.remove(&(-size, target.clone()));

            // Remove current hash from the list associated with `record_par_target`
            if let Some(records) = self.record_per_target.get_mut(&target) {
                records.remove(&(time, hash.clone()));
            }

            // Calculate new size
            size -= 1;

            if size == 0 {
                // If there are no elements remove entry associated with this peer
                self.record_per_target.remove(&target);
            } else {
                // otherwise, add this peer to `size_per_target` with new size
                self.size_per_target.insert((-size, target.clone()));
            }

            Some(target)
        } else {
            None
        }
    }

    pub fn insert(&mut self, hash: CryptoHash, target: PeerId) {
        self.remove_evicted();

        if self.main.contains_key(&hash) {
            return;
        }

        if self.is_full() {
            self.remove_frequent();
        }

        let now = Utc::now().timestamp_millis();

        self.main.insert(hash, (now, target.clone()));

        let mut size = self.record_per_target.get(&target).map_or(0, |x| x.len() as i64);

        if size > 0 {
            self.size_per_target.remove(&(-size, target.clone()));
        }

        self.record_per_target.entry(target.clone()).or_default().insert((now, hash));

        size += 1;
        self.size_per_target.insert((-size, target));
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use near_primitives::hash::hash;
    use std::{thread, time::Duration};

    /// Check internal state of the cache is ok
    fn check_consistency(cache: &RouteBackCache) {
        assert!(cache.main.len() as i64 <= cache.capacity);
        assert_eq!(cache.size_per_target.len(), cache.record_per_target.len());

        for (neg_size, target) in cache.size_per_target.iter() {
            let size = -neg_size;
            assert!(size > 0);
            assert_eq!(size, cache.record_per_target.get(&target).map(|x| x.len() as i64).unwrap());
        }

        let mut total = 0;

        for (target, records) in cache.record_per_target.iter() {
            total += records.len();

            for (time, record) in records.iter() {
                assert_eq!(cache.main.get(&record).unwrap(), &(*time, target.clone()));
            }
        }

        assert_eq!(cache.main.len(), total);
    }

    fn create_message(ix: u8) -> (PeerId, CryptoHash) {
        (PeerId::random(), hash(&[ix]))
    }

    #[test]
    fn simple() {
        let mut cache = RouteBackCache::new(100, 1000000000);
        let (peer0, hash0) = create_message(0);

        check_consistency(&cache);
        assert_eq!(cache.get(&hash0), None);
        cache.insert(hash0, peer0.clone());
        check_consistency(&cache);
        assert_eq!(cache.get(&hash0), Some(&peer0));
        assert_eq!(cache.remove(&hash0), Some(peer0.clone()));
        check_consistency(&cache);
        assert_eq!(cache.get(&hash0), None);
    }

    /// Check record is removed after some timeout.
    #[test]
    fn evicted() {
        let mut cache = RouteBackCache::new(1, 1);
        let (peer0, hash0) = create_message(0);

        cache.insert(hash0, peer0.clone());
        check_consistency(&cache);
        assert_eq!(cache.get(&hash0), Some(&peer0));
        thread::sleep(Duration::from_millis(2));
        cache.remove_evicted();
        check_consistency(&cache);
        assert_eq!(cache.get(&hash0), None);
    }

    /// Check element is removed after timeout triggered by insert at max capacity.
    #[test]
    fn insert_evicted() {
        let mut cache = RouteBackCache::new(1, 1);
        let (peer0, hash0) = create_message(0);
        let (peer1, hash1) = create_message(1);

        cache.insert(hash0, peer0.clone());
        check_consistency(&cache);
        assert_eq!(cache.get(&hash0), Some(&peer0));
        thread::sleep(Duration::from_millis(2));
        cache.insert(hash1, peer1.clone());
        check_consistency(&cache);
        assert_eq!(cache.get(&hash1), Some(&peer1));
        assert_eq!(cache.get(&hash0), None);
    }

    /// Check element is removed after insert because cache is at max capacity.
    #[test]
    fn insert_override() {
        let mut cache = RouteBackCache::new(1, 1000000000);
        let (peer0, hash0) = create_message(0);
        let (peer1, hash1) = create_message(1);

        cache.insert(hash0, peer0.clone());
        check_consistency(&cache);
        assert_eq!(cache.get(&hash0), Some(&peer0));
        thread::sleep(Duration::from_millis(2));
        cache.insert(hash1, peer1.clone());
        check_consistency(&cache);
        assert_eq!(cache.get(&hash1), Some(&peer1));
        assert_eq!(cache.get(&hash0), None);
    }

    /// Insert three elements. One old element from peer0 and two recent elements from peer1.
    /// Check that old element from peer0 is removed, even while peer1 has more elements.
    #[test]
    fn prefer_evict() {
        let mut cache = RouteBackCache::new(3, 50);
        let (peer0, hash0) = create_message(0);
        let (peer1, hash1) = create_message(1);
        let (_, hash2) = create_message(2);
        let (peer3, hash3) = create_message(3);

        cache.insert(hash0, peer0.clone());
        thread::sleep(Duration::from_millis(50));
        cache.insert(hash1, peer1.clone());
        cache.insert(hash2, peer1.clone());
        cache.insert(hash3, peer3.clone());
        check_consistency(&cache);

        assert!(cache.get(&hash0).is_none()); // This is removed, other exists
        assert!(cache.get(&hash1).is_some());
        assert!(cache.get(&hash2).is_some());
        assert!(cache.get(&hash3).is_some());
    }

    /// Insert three elements. One old element from peer0 and two recent elements from peer1.
    /// Check that older element from peer1 is removed, since evict timeout haven't passed yet.
    #[test]
    fn prefer_full() {
        let mut cache = RouteBackCache::new(3, 10000);
        let (peer0, hash0) = create_message(0);
        let (peer1, hash1) = create_message(1);
        let (_, hash2) = create_message(2);
        let (peer3, hash3) = create_message(3);

        cache.insert(hash0, peer0.clone());
        thread::sleep(Duration::from_millis(50));
        cache.insert(hash1, peer1.clone());
        cache.insert(hash2, peer1.clone());
        cache.insert(hash3, peer3.clone());
        check_consistency(&cache);

        assert!(cache.get(&hash0).is_some());
        assert!(cache.get(&hash1).is_none()); // This is removed, other exists
        assert!(cache.get(&hash2).is_some());
        assert!(cache.get(&hash3).is_some());
    }

    /// Simulate an attack from a malicious actor which sends several routing back message
    /// to overtake the cache. Create 4 legitimate hashes from 3 peers. Then insert
    /// 50 hashes from attacker. Since the cache size is 17, first 5 message from attacker will
    /// be stored, and it will become the peer with more entries (5 > 4). All 12 legitimate
    /// initial hashes should be present in the cache after the attack.
    #[test]
    fn poison_attack() {
        let mut cache = RouteBackCache::new(17, 1000000);
        let mut ix = 0;

        let mut peers = vec![];

        for _ in 0..3 {
            let peer = PeerId::random();

            for _ in 0..4 {
                let hashi = hash(&[ix]);
                ix += 1;
                cache.insert(hashi, peer.clone());
            }

            peers.push(peer);
        }

        let attacker = PeerId::random();

        for _ in 0..50 {
            let hashi = hash(&[ix]);
            ix += 1;
            cache.insert(hashi, attacker.clone());
        }

        check_consistency(&cache);

        ix = 0;

        for i in 0..3 {
            let peer = peers[i as usize].clone();

            for _ in 0..4 {
                let hashi = hash(&[ix]);
                ix += 1;
                assert_eq!(cache.get(&hashi), Some(&peer));
            }
        }
    }
}
