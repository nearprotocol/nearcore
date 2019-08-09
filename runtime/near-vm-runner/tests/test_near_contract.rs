use near_vm_logic::mocks::mock_external::MockedExternal;
use near_vm_logic::types::ReturnData;
use near_vm_logic::{Config, External, VMContext, VMOutcome};
use near_vm_runner::{run, VMError};
use std::fs;
use std::mem::size_of;
use std::path::PathBuf;

const CURRENT_ACCOUNT_ID: &str = "alice";
const SIGNER_ACCOUNT_ID: &str = "bob";
const SIGNER_ACCOUNT_PK: [u8; 3] = [0, 1, 2];
const PREDECESSOR_ACCOUNT_ID: &str = "carol";

fn create_context(input: &[u8]) -> VMContext {
    VMContext {
        current_account_id: CURRENT_ACCOUNT_ID.to_owned(),
        signer_account_id: SIGNER_ACCOUNT_ID.to_owned(),
        signer_account_pk: Vec::from(&SIGNER_ACCOUNT_PK[..]),
        predecessor_account_id: PREDECESSOR_ACCOUNT_ID.to_owned(),
        input: Vec::from(input),
        block_index: 0,
        account_balance: 0,
        attached_deposit: 0,
        prepaid_gas: 10u64.pow(9),
        random_seed: vec![0, 1, 2],
        free_of_charge: false,
    }
}

#[test]
pub fn test_ts_contract() {
    let mut path = PathBuf::from("/tmp/main.wasm");
    let code = fs::read(path).unwrap();
    let mut fake_external = MockedExternal::new();

    let context = create_context(&[]);
    let config = Config::default();

    // Call method that panics.
    let promise_results = vec![];

    // Call method that reads the value from storage using registers.
    let context = create_context(b"{\"name\":\"Alice\"}");
    let result =
        run(vec![], &code, b"hello", &mut fake_external, context, &config, &promise_results);

    if let ReturnData::Value(value) = result.unwrap().return_data {
        let value = String::from_utf8(value).unwrap();
        assert_eq!(value, "helloAlice");
    } else {
        panic!("Value was not returned");
    }
}
