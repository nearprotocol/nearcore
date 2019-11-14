use std::collections::{HashMap, HashSet, VecDeque};

use crate::types::{AccountPK, DrainingIterator, TransactionGroup};
use near_primitives::hash::CryptoHash;
use near_primitives::transaction::SignedTransaction;

pub mod types;

/// Transaction pool: keeps track of transactions that were not yet accepted into the block chain.
#[derive(Default)]
pub struct TransactionPool {
    /// Transactions are grouped by a pair of (account ID, signer public key).
    /// NOTE: It's more efficient on average to keep transactions unsorted and with potentially
    /// conflicting nonce than to create a BTreeMap for every transaction.
    pub transactions: HashMap<AccountPK, Vec<SignedTransaction>>,
    /// Set of all hashes to quickly check if the given transaction is in the pool.
    pub unique_transactions: HashSet<CryptoHash>,
}

impl TransactionPool {
    /// Insert a signed transaction into the pool that passed validation.
    pub fn insert_transaction(&mut self, signed_transaction: SignedTransaction) {
        if self.unique_transactions.contains(&signed_transaction.get_hash()) {
            return;
        }
        self.unique_transactions.insert(signed_transaction.get_hash());
        let signer_id = signed_transaction.transaction.signer_id.clone();
        let signer_public_key = signed_transaction.transaction.public_key.clone();
        self.transactions
            .entry((signer_id, signer_public_key))
            .or_insert_with(Vec::new)
            .push(signed_transaction);
    }

    /// Returns a draining structure that pulls transaction groups with the same key from the pool
    /// in the proper order defined by the protocol. When the iterator is dropped, all remaining
    /// transaction groups are inserted back into the pool.
    pub fn draining_iterator(&mut self) -> PoolIterator {
        PoolIterator::new(self)
    }

    /// Quick reconciliation step - evict all transactions that already in the block
    /// or became invalid after it.
    pub fn remove_transactions(&mut self, transactions: &[SignedTransaction]) {
        let mut grouped_transactions = HashMap::new();
        for tx in transactions {
            if self.unique_transactions.contains(&tx.get_hash()) {
                let signer_id = &tx.transaction.signer_id;
                let signer_public_key = &tx.transaction.public_key;
                grouped_transactions
                    .entry((signer_id, signer_public_key))
                    .or_insert_with(HashSet::new)
                    .insert(tx.get_hash());
            }
        }
        for (key, hashes) in grouped_transactions {
            let key = (key.0.clone(), key.1.clone());
            let mut remove_entry = false;
            if let Some(v) = self.transactions.get_mut(&key) {
                v.retain(|tx| !hashes.contains(&tx.get_hash()));
                remove_entry = v.is_empty();
            }
            if remove_entry {
                self.transactions.remove(&key);
            }
            for hash in hashes {
                self.unique_transactions.remove(&hash);
            }
        }
    }

    /// Reintroduce transactions back during the chain reorg
    pub fn reintroduce_transactions(&mut self, transactions: Vec<SignedTransaction>) {
        for tx in transactions {
            self.insert_transaction(tx);
        }
    }

    pub fn len(&self) -> usize {
        self.unique_transactions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.unique_transactions.is_empty()
    }
}

/// PoolIterator is a structure to pull transactions from the pool.
/// It implements `DrainingIterator` trait that returns a transaction groups one by one.
/// When a draining iterator is dropped the remaining transactions are returned back to the pool.
pub struct PoolIterator<'a> {
    /// Mutable reference to the pool, to avoid exposing it while the iterator exists.
    pool: &'a mut TransactionPool,

    /// Queue of transaction groups. Each group there is sorted by nonce.
    sorted_groups: VecDeque<TransactionGroup>,
}

impl<'a> PoolIterator<'a> {
    pub fn new(pool: &'a mut TransactionPool) -> Self {
        Self { pool, sorted_groups: Default::default() }
    }
}

/// The iterator works with the following algorithm:
/// On next(), the iterator tries to pulls a transaction group from the pool, sorts transactions in
/// it, and add it to the back of the sorted groups queue.
///
/// If the pool is empty, the iterator pulls group from the front of the sorted groups queue.
///
/// If this pulled group is empty (no transactions left inside), then the discards the iterator
/// updates `unique_transactions` in the pool and discards the group. Then pulls the next one.
///
/// Once a non-empty group found, the group is pushed back to the back of the sorted groups
/// queue and the iterator returns a mutable reference to this group.
///
/// If the sorted groups queue is empty, the iterator returns None.
///
/// When the iterator is dropped, `unique_transactions` in the pool is updated for every group.
/// And all non-empty group from the sorted groups queue are inserted back into the pool.
impl<'a> DrainingIterator for PoolIterator<'a> {
    fn next(&mut self) -> Option<&mut TransactionGroup> {
        let key = self.pool.transactions.keys().next().cloned();
        match key {
            Some(key) => {
                let mut transactions =
                    self.pool.transactions.remove(&key.clone()).expect("just checked existence");
                transactions.sort_by_key(|st| std::cmp::Reverse(st.transaction.nonce));
                self.sorted_groups.push_back(TransactionGroup {
                    key,
                    transactions,
                    removed_transaction_hashes: vec![],
                });
                Some(self.sorted_groups.back_mut().expect("just pushed"))
            }
            None => {
                while let Some(sorted_group) = self.sorted_groups.pop_front() {
                    if sorted_group.transactions.is_empty() {
                        for hash in sorted_group.removed_transaction_hashes {
                            self.pool.unique_transactions.remove(&hash);
                        }
                    } else {
                        self.sorted_groups.push_back(sorted_group);
                        return Some(self.sorted_groups.back_mut().expect("just pushed"));
                    }
                }
                None
            }
        }
    }
}

/// When a pool iterator is dropped, all remaining non empty transaction groups from the sorted
/// groups queue are inserted back into the pool. And removed transactions hashes from groups are
/// removed from the pool's unique_transactions.
impl<'a> Drop for PoolIterator<'a> {
    fn drop(&mut self) {
        for group in self.sorted_groups.drain(..) {
            for hash in group.removed_transaction_hashes {
                self.pool.unique_transactions.remove(&hash);
            }
            if !group.transactions.is_empty() {
                self.pool.transactions.insert(group.key, group.transactions);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use rand::seq::SliceRandom;
    use rand::thread_rng;

    use near_crypto::{InMemorySigner, KeyType};

    use near_primitives::hash::CryptoHash;
    use near_primitives::types::Balance;

    fn generate_transactions(
        signer_id: &str,
        signer_seed: &str,
        starting_nonce: u64,
        end_nonce: u64,
    ) -> Vec<SignedTransaction> {
        let signer =
            Arc::new(InMemorySigner::from_seed(signer_seed, KeyType::ED25519, signer_seed));
        (starting_nonce..=end_nonce)
            .map(|i| {
                SignedTransaction::send_money(
                    i,
                    signer_id.to_string(),
                    "bob.near".to_string(),
                    &*signer,
                    i as Balance,
                    CryptoHash::default(),
                )
            })
            .collect()
    }

    fn process_txs_to_nonces(
        mut transactions: Vec<SignedTransaction>,
        expected_weight: u32,
    ) -> (Vec<u64>, TransactionPool) {
        let mut pool = TransactionPool::default();
        let mut rng = thread_rng();
        transactions.shuffle(&mut rng);
        for tx in transactions {
            pool.insert_transaction(tx);
        }
        (
            prepare_transactions(&mut pool, expected_weight)
                .iter()
                .map(|tx| tx.transaction.nonce)
                .collect(),
            pool,
        )
    }

    fn sort_pairs(a: &mut [u64]) {
        for c in a.chunks_exact_mut(2) {
            if c[0] > c[1] {
                c.swap(0, 1);
            }
        }
    }

    fn prepare_transactions(
        pool: &mut TransactionPool,
        max_number_of_transactions: u32,
    ) -> Vec<SignedTransaction> {
        let mut res = vec![];
        let mut pool_iter = pool.draining_iterator();
        while res.len() < max_number_of_transactions as usize {
            if let Some(iter) = pool_iter.next() {
                if let Some(tx) = iter.next() {
                    res.push(tx);
                }
            } else {
                break;
            }
        }
        res
    }

    /// Add transactions of nonce from 1..10 in random order. Check that mempool
    /// orders them correctly.
    #[test]
    fn test_order_nonce() {
        let transactions = generate_transactions("alice.near", "alice.near", 1, 10);
        let (nonces, _) = process_txs_to_nonces(transactions, 10);
        assert_eq!(nonces, (1..=10).collect::<Vec<u64>>());
    }

    /// Add transactions of nonce from 1..10 in random order from 2 signers. Check that mempool
    /// orders them correctly.
    #[test]
    fn test_order_nonce_two_signers() {
        let mut transactions = generate_transactions("alice.near", "alice.near", 1, 10);
        transactions.extend(generate_transactions("bob.near", "bob.near", 1, 10));

        let (nonces, _) = process_txs_to_nonces(transactions, 10);
        assert_eq!(nonces, (1..=5).map(|a| vec![a; 2]).flatten().collect::<Vec<u64>>());
    }

    /// Add transactions of nonce from 1..10 in random order from the same account but with
    /// different public keys.
    #[test]
    fn test_order_nonce_same_account_two_access_keys_variable_nonces() {
        let mut transactions = generate_transactions("alice.near", "alice.near", 1, 10);
        transactions.extend(generate_transactions("alice.near", "bob.near", 21, 30));

        let (mut nonces, _) = process_txs_to_nonces(transactions, 10);
        sort_pairs(&mut nonces[..]);
        assert_eq!(nonces, (1..=5).map(|a| vec![a, a + 20]).flatten().collect::<Vec<u64>>());
    }

    /// Add transactions of nonce from 1..=3 and transactions with nonce 21..=31. Pull 10.
    /// Then try to get another 10.
    #[test]
    fn test_retain() {
        let mut transactions = generate_transactions("alice.near", "alice.near", 1, 3);
        transactions.extend(generate_transactions("alice.near", "bob.near", 21, 31));

        let (mut nonces, mut pool) = process_txs_to_nonces(transactions, 10);
        sort_pairs(&mut nonces[..6]);
        assert_eq!(nonces, vec![1, 21, 2, 22, 3, 23, 24, 25, 26, 27]);
        let nonces: Vec<u64> =
            prepare_transactions(&mut pool, 10).iter().map(|tx| tx.transaction.nonce).collect();
        assert_eq!(nonces, vec![28, 29, 30, 31]);
    }

    #[test]
    fn test_remove_transactions() {
        let n = 100;
        let mut transactions = (1..=n)
            .map(|i| {
                let signer_seed = format!("user_{}", i % 3);
                let signer = Arc::new(InMemorySigner::from_seed(
                    &signer_seed,
                    KeyType::ED25519,
                    &signer_seed,
                ));
                let signer_id = format!("user_{}", i % 5);
                SignedTransaction::send_money(
                    i,
                    signer_id.to_string(),
                    "bob.near".to_string(),
                    &*signer,
                    i as Balance,
                    CryptoHash::default(),
                )
            })
            .collect::<Vec<_>>();

        let mut pool = TransactionPool::default();
        let mut rng = thread_rng();
        transactions.shuffle(&mut rng);
        for tx in transactions.clone() {
            println!("{:?}", tx);
            pool.insert_transaction(tx);
        }
        assert_eq!(pool.len(), n as usize);

        transactions.shuffle(&mut rng);
        let (txs_to_remove, txs_to_check) = transactions.split_at(transactions.len() / 2);
        pool.remove_transactions(txs_to_remove);

        assert_eq!(pool.len(), txs_to_check.len());

        let mut pool_txs = prepare_transactions(&mut pool, txs_to_check.len() as u32);
        pool_txs.sort_by_key(|tx| tx.transaction.nonce);
        let mut expected_txs = txs_to_check.to_vec();
        expected_txs.sort_by_key(|tx| tx.transaction.nonce);

        assert_eq!(pool_txs, expected_txs);
    }

    /// Add transactions of nonce from 1..=3 and transactions with nonce 21..=31. Pull 10.
    /// Then try to get another 10.
    #[test]
    fn test_draining_iterator() {
        let mut transactions = generate_transactions("alice.near", "alice.near", 1, 3);
        transactions.extend(generate_transactions("alice.near", "bob.near", 21, 31));

        let (nonces, mut pool) = process_txs_to_nonces(transactions, 0);
        assert!(nonces.is_empty());
        let mut res = vec![];
        let mut pool_iter = pool.draining_iterator();
        while let Some(iter) = pool_iter.next() {
            while let Some(tx) = iter.next() {
                if tx.transaction.nonce & 1 == 1 {
                    res.push(tx);
                    break;
                }
            }
        }
        let mut nonces: Vec<_> = res.into_iter().map(|tx| tx.transaction.nonce).collect();
        sort_pairs(&mut nonces[..4]);
        assert_eq!(nonces, vec![1, 21, 3, 23, 25, 27, 29, 31]);
    }
}
