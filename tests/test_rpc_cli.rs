#[macro_use]
extern crate lazy_static;
extern crate primitives;
extern crate serde_json;
extern crate service;

use serde_json::Value;
use service::rpc::types::{
    CallViewFunctionResponse, ViewAccountResponse,
};
use service::test_utils::run_test_service;
use std::borrow::Cow;
use std::process::{Command, Output};
use std::thread;
use primitives::signature::get_keypair;
use std::fs;
use std::path::Path;

fn test_service_ready() -> bool {
    thread::spawn(|| run_test_service());
    true
}

fn get_key_path() -> String {
    let key_path = Path::new("/tmp/near_key");
    if !key_path.exists() {
        let (_, secret_key) = get_keypair();
        fs::write(key_path, secret_key.to_string()).unwrap();
    }
    key_path.to_string_lossy().into_owned()
}

lazy_static! {
    static ref DEVNET_STARTED: bool = test_service_ready();
    static ref KEY_PATH: String = get_key_path();
}

fn check_result(output: &Output) -> Cow<str> {
    if !output.status.success() {
        panic!("{}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8_lossy(&output.stdout)
}

#[test]
fn test_send_money() {
    if !*DEVNET_STARTED { panic!() }
    let output = Command::new("./scripts/rpc.py")
        .arg("send_money")
        .arg("-p")
        .arg(&*KEY_PATH)
        .output()
        .expect("send_money command failed to process");
    let result = check_result(&output);
    let data: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(data, Value::Null);
}

#[test]
fn test_view_account() {
    if !*DEVNET_STARTED { panic!() }
    let output = Command::new("./scripts/rpc.py")
        .arg("view_account")
        .output()
        .expect("view_account command failed to process");
    let result = check_result(&output);
    let _: ViewAccountResponse = serde_json::from_str(&result).unwrap();
}

#[test]
fn test_deploy() {
    if !*DEVNET_STARTED { panic!() }
    let output = Command::new("./scripts/rpc.py")
        .arg("deploy")
        .arg("test_contract_name")
        .arg("core/wasm/runtest/res/wasm_with_mem.wasm")
        .arg("-p")
        .arg(&*KEY_PATH)
        .output()
        .expect("deploy command failed to process");
    let result = check_result(&output);
    let data: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(data, Value::Null);
}

#[test]
fn test_schedule_function_call() {
    if !*DEVNET_STARTED { panic!() }
    test_deploy();
    let output = Command::new("./scripts/rpc.py")
        .arg("schedule_function_call")
        .arg("test_contract_name")
        .arg("run_test")
        .arg("-p")
        .arg(&*KEY_PATH)
        .output()
        .expect("schedule_function_call command failed to process");
    let result = check_result(&output);
    let data: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(data, Value::Null);
}

#[test]
fn test_call_view_function() {
    if !*DEVNET_STARTED { panic!() }
    test_deploy();
    let output = Command::new("./scripts/rpc.py")
        .arg("call_view_function")
        .arg("test_contract_name")
        .arg("run_test")
        .output()
        .expect("call_view_function command failed to process");
    let result = check_result(&output);
    let _: CallViewFunctionResponse = serde_json::from_str(&result).unwrap();
}
