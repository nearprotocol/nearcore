use near_runtime_fees::RuntimeFeesConfig;
use near_vm_errors::FunctionCallError;
use near_vm_logic::mocks::mock_external::MockedExternal;
use near_vm_logic::types::ReturnData;
use near_vm_logic::{External, HostError, VMConfig, VMContext};
use near_vm_runner::{run, VMError};
use std::fs;
use std::path::PathBuf;

mod utils;

fn create_context(input: &[u8]) -> VMContext {
    let input = input.to_vec();
    crate::utils::create_context(input)
}

#[test]
pub fn test_ts_contract() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/res/test_contract_ts.wasm");
    let code = fs::read(path).unwrap();
    let mut fake_external = MockedExternal::new();

    let context = create_context(&[]);
    let config = VMConfig::default();
    let fees = RuntimeFeesConfig::default();

    // Call method that panics.
    let promise_results = vec![];
    let result = run(
        vec![],
        &code,
        b"try_panic",
        &mut fake_external,
        context,
        &config,
        &fees,
        &promise_results,
    );
    assert_eq!(
        result.1,
        Some(VMError::FunctionCallError(FunctionCallError::HostError(HostError::GuestPanic(
            "explicit guest panic".to_string()
        ))))
    );

    // Call method that writes something into storage.
    let context = create_context(b"foo bar");
    run(
        vec![],
        &code,
        b"try_storage_write",
        &mut fake_external,
        context,
        &config,
        &fees,
        &promise_results,
    )
    .0
    .unwrap();
    // Verify by looking directly into the storage of the host.
    let res = fake_external.storage_get(b"foo");
    let value_ptr = res.unwrap().unwrap();
    let value = value_ptr.deref_box().unwrap();
    let value = String::from_utf8(value).unwrap();
    assert_eq!(value.as_str(), "bar");

    // Call method that reads the value from storage using registers.
    let context = create_context(b"foo");
    let result = run(
        vec![],
        &code,
        b"try_storage_read",
        &mut fake_external,
        context,
        &config,
        &fees,
        &promise_results,
    );

    if let ReturnData::Value(value) = result.0.unwrap().return_data {
        let value = String::from_utf8(value).unwrap();
        assert_eq!(value, "bar");
    } else {
        panic!("Value was not returned");
    }
}
