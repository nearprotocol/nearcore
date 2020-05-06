use near_pool::{types::PoolIterator, TransactionPool};
use near_primitives::{
    account::Account,
    errors::RuntimeError,
    hash::CryptoHash,
    receipt::Receipt,
    state_record::StateRecord,
    transaction::{ExecutionOutcome, ExecutionStatus, SignedTransaction},
    types::AccountId,
    types::{Balance, BlockHeight, Gas},
};
use near_runtime_configs::RuntimeConfig;
use near_store::{get_account, Store, Trie, TrieUpdate};
use node_runtime::{ApplyState, Runtime};

use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct GenesisConfig {
    pub genesis_time: u64,
    pub gas_price: Balance,
    pub gas_limit: Gas,
    pub runtime_config: RuntimeConfig,
    pub state_records: Vec<StateRecord>,
}

#[derive(Debug, Default, Clone)]
struct Block {
    prev_block: Option<Box<Block>>,
    pub state_root: CryptoHash,
    transactions: Vec<SignedTransaction>,
    receipts: Vec<Receipt>,
    block_height: BlockHeight,
    block_timestamp: u64,
    gas_price: Balance,
    gas_limit: Gas,
    gas_burnt: Gas,
}

impl Block {
    pub fn genesis(genesis_config: &GenesisConfig) -> Self {
        Self {
            prev_block: None,
            state_root: CryptoHash::default(),
            transactions: vec![],
            receipts: vec![],
            block_height: 1,
            block_timestamp: genesis_config.genesis_time,
            gas_price: genesis_config.gas_price,
            gas_limit: genesis_config.gas_limit,
            gas_burnt: 0,
        }
    }

    pub fn produce(&self, new_state_root: CryptoHash) -> Block {
        Self {
            gas_price: self.gas_price,
            gas_limit: self.gas_limit,
            block_timestamp: self.block_timestamp + 1,
            prev_block: Some(Box::new(self.clone())),
            state_root: new_state_root,
            transactions: vec![],
            receipts: vec![],
            block_height: 1,
            gas_burnt: 0,
        }
    }
}

pub struct RuntimeStandalone {
    tx_pool: TransactionPool,
    transactions: HashMap<CryptoHash, SignedTransaction>,
    outcomes: HashMap<CryptoHash, ExecutionOutcome>,
    cur_block: Block,
    runtime: Runtime,
    trie: Arc<Trie>,
    pending_receipts: Vec<Receipt>,
}

impl RuntimeStandalone {
    pub fn new(genesis: GenesisConfig, store: Arc<Store>) -> Self {
        let mut genesis_block = Block::genesis(&genesis);
        let mut store_update = store.store_update();
        let runtime = Runtime::new(genesis.runtime_config.clone());
        let trie = Arc::new(Trie::new(store));
        let trie_update = TrieUpdate::new(trie.clone(), CryptoHash::default());
        let (s_update, state_root) =
            runtime.apply_genesis_state(trie_update, &[], &genesis.state_records);
        store_update.merge(s_update);
        store_update.commit().unwrap();
        genesis_block.state_root = state_root;
        Self {
            trie,
            runtime,
            transactions: HashMap::new(),
            outcomes: HashMap::new(),
            cur_block: genesis_block,
            tx_pool: TransactionPool::new(),
            pending_receipts: vec![],
        }
    }

    pub fn run_tx(&mut self, mut tx: SignedTransaction) -> Result<ExecutionOutcome, RuntimeError> {
        tx.init();
        let tx_hash = tx.get_hash();
        self.transactions.insert(tx_hash, tx.clone());
        self.tx_pool.insert_transaction(tx);
        self.process_block()?;
        Ok(self
            .outcomes
            .get(&tx_hash)
            .expect("successful self.process() guaranies to have outcome for a tx")
            .clone())
    }

    /// Processes blocks until the final value is produced
    pub fn resolve_tx(
        &mut self,
        mut tx: SignedTransaction,
    ) -> Result<ExecutionOutcome, RuntimeError> {
        tx.init();
        let mut tx_hash = tx.get_hash();
        self.transactions.insert(tx_hash, tx.clone());
        self.tx_pool.insert_transaction(tx);
        loop {
            self.process_block()?;
            let outcome = self.outcomes.get(&tx_hash).unwrap();
            match outcome.status {
                ExecutionStatus::SuccessReceiptId(ref id) => tx_hash = *id,
                ExecutionStatus::SuccessValue(_)
                | ExecutionStatus::Failure(_)
                | ExecutionStatus::Unknown => return Ok(outcome.clone()),
            };
        }
    }

    /// Processes all transactions and pending receipts until there is no pending_receipts left
    pub fn run_all(&mut self) -> Result<(), RuntimeError> {
        loop {
            self.process_block()?;
            if self.pending_receipts.len() == 0 {
                return Ok(());
            }
        }
    }

    /// Processes one block. Populates outcomes and producining new pending_receipts.
    pub fn process_block(&mut self) -> Result<(), RuntimeError> {
        let apply_state = ApplyState {
            block_index: self.cur_block.block_height,
            epoch_length: 0, // TODO: support for epochs
            epoch_height: self.cur_block.block_height,
            gas_price: self.cur_block.gas_price,
            block_timestamp: self.cur_block.block_timestamp,
            gas_limit: None,
        };

        let apply_result = self.runtime.apply(
            self.trie.clone(),
            self.cur_block.state_root,
            &None,
            &apply_state,
            &self.pending_receipts,
            &Self::prepare_transactions(&mut self.tx_pool),
        )?;
        self.pending_receipts = apply_result.outgoing_receipts;
        apply_result.outcomes.iter().for_each(|outcome| {
            self.outcomes.insert(outcome.id, outcome.outcome.clone());
        });
        let (update, _) =
            apply_result.trie_changes.into(self.trie.clone()).expect("Unexpected Storage error");
        update.commit().expect("Unexpected io error");
        self.cur_block = self.cur_block.produce(apply_result.state_root);

        Ok(())
    }

    pub fn view_account(&self, account_id: &AccountId) -> Option<Account> {
        let trie_update = TrieUpdate::new(self.trie.clone(), self.cur_block.state_root);
        get_account(&trie_update, &account_id).expect("Unexpected Storage error")
    }

    pub fn pending_receipts(&self) -> &[Receipt] {
        &self.pending_receipts
    }

    fn prepare_transactions(tx_pool: &mut TransactionPool) -> Vec<SignedTransaction> {
        let mut res = vec![];
        let mut pool_iter = tx_pool.pool_iterator();
        loop {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_crypto::{InMemorySigner, KeyType, Signer};
    use near_primitives::{account::AccessKey, test_utils::account_new};
    use near_store::test_utils::create_test_store;

    // Inits runtime with
    fn init_runtime(signer: &InMemorySigner) -> RuntimeStandalone {
        let mut genesis = GenesisConfig::default();
        let root_account = account_new(std::u128::MAX, CryptoHash::default());

        genesis.state_records.push(StateRecord::Account {
            account_id: signer.account_id.clone(),
            account: root_account,
        });
        genesis.state_records.push(StateRecord::AccessKey {
            account_id: signer.account_id.clone(),
            public_key: signer.public_key(),
            access_key: AccessKey::full_access(),
        });

        RuntimeStandalone::new(genesis, create_test_store())
    }

    #[test]
    fn single_block() {
        let signer = InMemorySigner::from_seed("bob".into(), KeyType::ED25519, "test");

        let mut runtime = init_runtime(&signer);
        let outcome = runtime.run_tx(SignedTransaction::create_account(
            1,
            signer.account_id.clone(),
            "alice".into(),
            100,
            signer.public_key(),
            &signer,
            CryptoHash::default(),
        ));
        assert!(matches!(
            outcome,
            Ok(ExecutionOutcome { status: ExecutionStatus::SuccessReceiptId(_), .. })
        ));
    }

    #[test]
    fn run_all() {
        let signer = InMemorySigner::from_seed("bob".into(), KeyType::ED25519, "test");
        let mut runtime = init_runtime(&signer);
        assert_eq!(runtime.view_account(&"alice".into()), None);
        let outcome = runtime.resolve_tx(SignedTransaction::create_account(
            1,
            signer.account_id.clone(),
            "alice".into(),
            165437999999999999999000,
            signer.public_key(),
            &signer,
            CryptoHash::default(),
        ));
        assert!(matches!(
            outcome,
            Ok(ExecutionOutcome { status: ExecutionStatus::SuccessValue(_), .. })
        ));
        assert_eq!(
            runtime.view_account(&"alice".into()),
            Some(Account {
                amount: 165437999999999999999000,
                code_hash: CryptoHash::default(),
                locked: 0,
                storage_usage: 182,
            })
        );
    }

    #[test]
    fn test_cross_contract_call() {
        let signer = InMemorySigner::from_seed("bob".into(), KeyType::ED25519, "test");
        let mut runtime = init_runtime(&signer);

        assert!(matches!(
            runtime.resolve_tx(SignedTransaction::create_contract(
                1,
                signer.account_id.clone(),
                "status".into(),
                include_bytes!("../contracts/status-message/res/status_message.wasm")
                    .as_ref()
                    .into(),
                23082408900000000000001000,
                signer.public_key(),
                &signer,
                CryptoHash::default(),
            )),
            Ok(ExecutionOutcome { status: ExecutionStatus::SuccessValue(_), .. })
        ));

        assert!(matches!(
            runtime.resolve_tx(SignedTransaction::create_contract(
                2,
                signer.account_id.clone(),
                "caller".into(),
                include_bytes!(
                    "../contracts/cross-contract-high-level/res/cross_contract_high_level.wasm"
                )
                .as_ref()
                .into(),
                23082408900000000000001000,
                signer.public_key(),
                &signer,
                CryptoHash::default(),
            )),
            Ok(ExecutionOutcome { status: ExecutionStatus::SuccessValue(_), .. })
        ));

        assert!(matches!(
            runtime.resolve_tx(SignedTransaction::call(
                3,
                signer.account_id.clone(),
                "caller".into(),
                &signer,
                0,
                "simple_call".into(),
                "{\"account_id\": \"status\", \"message\": \"forwarded msg\"}".as_bytes().to_vec(),
                10000000000000000,
                CryptoHash::default(),
            )),
            Ok(ExecutionOutcome { status: ExecutionStatus::SuccessValue(_), .. })
        ));
    }
}
