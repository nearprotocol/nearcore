use std::collections::BTreeMap;

use near_primitives::types::{AccountId, Balance, PromiseId, ReceiptId};
use wasm::ext::{Error as ExtError, External, Result as ExtResult};

#[derive(Default)]
struct MyExt {
    storage: BTreeMap<Vec<u8>, Vec<u8>>,
    num_receipts: u32,
}

fn generate_promise_id(index: u32) -> ReceiptId {
    [index as u8; 32].to_vec()
}

impl External for MyExt {
    fn storage_set(&mut self, key: &[u8], value: &[u8]) -> ExtResult<Option<Vec<u8>>> {
        println!("PUT '{:?}' -> '{:?}'", key, value);
        let evicted = self.storage.insert(Vec::from(key), Vec::from(value));
        if let Some(evicted) = evicted.as_ref() {
            println!("EVICTED '{:?}' -> '{:?}'", key, evicted);
        }
        Ok(evicted)
    }

    fn storage_get(&self, key: &[u8]) -> ExtResult<Option<Vec<u8>>> {
        let value = self.storage.get(key);
        match value {
            Some(buf) => {
                println!("GET '{:?}' -> '{:?}'", key, buf);
                Ok(Some(buf.to_vec()))
            }
            None => {
                println!("GET '{:?}' -> EMPTY", key);
                Ok(None)
            }
        }
    }

    fn storage_remove(&mut self, key: &[u8]) -> ExtResult<Option<Vec<u8>>> {
        let removed = self.storage.remove(key);
        if let Some(removed) = removed.as_ref() {
            println!("REMOVE '{:?}' -> '{:?}'", key, removed);
        } else {
            println!("REMOVE '{:?}' -> EMPTY", key);
        }
        Ok(removed)
    }

    fn storage_iter(&mut self, _prefix: &[u8]) -> ExtResult<u32> {
        Err(ExtError::NotImplemented)
    }

    fn storage_range(&mut self, _start: &[u8], _end: &[u8]) -> ExtResult<u32> {
        Err(ExtError::NotImplemented)
    }

    fn storage_iter_next(&mut self, _iter: u32) -> ExtResult<Option<Vec<u8>>> {
        Err(ExtError::NotImplemented)
    }

    fn storage_iter_peek(&mut self, _iter: u32) -> ExtResult<Option<Vec<u8>>> {
        Err(ExtError::NotImplemented)
    }

    fn storage_iter_remove(&mut self, _iter: u32) {}

    fn promise_create(
        &mut self,
        account_id: AccountId,
        _method_name: Vec<u8>,
        _arguments: Vec<u8>,
        _amount: Balance,
    ) -> ExtResult<PromiseId> {
        match self.num_receipts {
            0 => assert_eq!(&account_id, &"test1".to_string()),
            1 => assert_eq!(&account_id, &"test2".to_string()),
            _ => (),
        };
        self.num_receipts += 1;
        Ok(PromiseId::Receipt(generate_promise_id(self.num_receipts - 1)))
    }

    fn promise_then(
        &mut self,
        promise_id: PromiseId,
        _method_name: Vec<u8>,
        _arguments: Vec<u8>,
        _amount: Balance,
    ) -> ExtResult<PromiseId> {
        match promise_id {
            PromiseId::Receipt(_) => Err(ExtError::WrongPromise),
            PromiseId::Joiner(v) => {
                assert_eq!(v[0], generate_promise_id(0));
                assert_eq!(v[1], generate_promise_id(1));
                Ok(PromiseId::Callback(b"call_it_please".to_vec()))
            }
            _ => Err(ExtError::WrongPromise),
        }
    }

    fn check_ethash(
        &mut self,
        _block_number: u64,
        _header_hash: &[u8],
        _nonce: u64,
        _mix_hash: &[u8],
        _difficulty: u64,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use byteorder::{ByteOrder, LittleEndian};

    use near_primitives::hash::hash;
    use near_primitives::types::StorageUsage;
    use wasm::executor::{self, ExecutionOutcome};
    use wasm::types::{Config, ContractCode, Error, ReturnData, RuntimeContext};

    use super::*;

    fn run_with_filename(
        method_name: &[u8],
        input_data: &[u8],
        result_data: &[Option<Vec<u8>>],
        context: &RuntimeContext,
        filename: &str,
    ) -> Result<ExecutionOutcome, Error> {
        let wasm_binary = fs::read(filename).expect("Unable to read file");
        let code = ContractCode::new(wasm_binary);

        let mut ext = MyExt::default();
        let config = Config::default();

        executor::execute(
            &code,
            &method_name,
            &input_data,
            &result_data,
            &mut ext,
            &config,
            &context,
        )
    }

    fn run(
        method_name: &[u8],
        input_data: &[u8],
        result_data: &[Option<Vec<u8>>],
        context: &RuntimeContext,
    ) -> Result<ExecutionOutcome, Error> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("res/wasm_with_mem.wasm");
        run_with_filename(method_name, input_data, result_data, context, path.to_str().unwrap())
    }

    fn encode_i32(val: i32) -> [u8; 4] {
        let mut tmp = [0u8; 4];
        LittleEndian::write_i32(&mut tmp, val);
        tmp
    }

    fn decode_i32(val: &[u8]) -> i32 {
        LittleEndian::read_i32(val)
    }

    fn decode_u64(val: &[u8]) -> u64 {
        LittleEndian::read_u64(val)
    }

    fn runtime_context(
        balance: Balance,
        amount: Balance,
        storage_usage: StorageUsage,
    ) -> RuntimeContext {
        RuntimeContext::new(
            balance,
            amount,
            &"alice.near".to_string(),
            &"bob".to_string(),
            storage_usage,
            123,
            b"yolo".to_vec(),
            false,
        )
    }

    #[test]
    fn test_storage() {
        let input_data = [0u8; 0];

        let return_data = run(b"run_test", &input_data, &[], &runtime_context(0, 1_000_000, 0))
            .map(|outcome| outcome.return_data)
            .expect("ok");

        match return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(decode_i32(&output_data), 10),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_input() {
        let input_data = [10u8, 0, 0, 0, 30u8, 0, 0, 0];

        let return_data =
            run(b"sum_with_input", &input_data, &[], &runtime_context(0, 1_000_000_000, 0))
                .map(|outcome| outcome.return_data)
                .expect("ok");

        match return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(decode_i32(&output_data), 40),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_result_ok() {
        let input_data = [0u8; 0];
        let result_data = vec![
            Some(encode_i32(2).to_vec()),
            Some(encode_i32(4).to_vec()),
            Some(encode_i32(6).to_vec()),
        ];

        let return_data = run(
            b"sum_with_multiple_results",
            &input_data,
            &result_data,
            &runtime_context(0, 1_000_000_000, 0),
        )
        .map(|outcome| outcome.return_data)
        .expect("ok");

        match return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(decode_i32(&output_data), 12),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_promises() {
        let input_data = [0u8; 0];

        let outcome =
            run(b"create_promises_and_join", &input_data, &[], &runtime_context(0, 1_000_000, 0))
                .expect("ok");

        match outcome.return_data {
            Ok(ReturnData::Promise(promise_id)) => {
                assert_eq!(&promise_id, &PromiseId::Callback(b"call_it_please".to_vec()))
            }
            _ => assert!(false, "Expected returned promise"),
        };
    }

    #[test]
    fn test_assert_sum_ok() {
        let input_data = [10u8, 0, 0, 0, 30u8, 0, 0, 0, 40u8, 0, 0, 0];

        run(b"assert_sum", &input_data, &[], &runtime_context(0, 0, 0)).expect("ok");
    }

    #[test]
    fn test_assert_sum_fail() {
        let input_data = [10u8, 0, 0, 0, 30u8, 0, 0, 0, 45u8, 0, 0, 0];

        let outcome = run(b"assert_sum", &input_data, &[], &runtime_context(0, 0, 0))
            .expect("outcome to be ok");

        match outcome.return_data {
            Err(_) => assert!(true, "That's legit"),
            _ => assert!(false, "Expected to fail with assert failure"),
        };
    }

    #[test]
    fn test_frozen_balance() {
        let input_data = [0u8; 0];

        let outcome =
            run(b"get_frozen_balance", &input_data, &[], &runtime_context(10, 100, 0)).expect("ok");

        // The frozen balance is not used for the runtime deductions.
        match outcome.return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(decode_u64(&output_data), 10),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_liquid_balance() {
        let input_data = [0u8; 0];

        let outcome =
            run(b"get_liquid_balance", &input_data, &[], &runtime_context(0, 100, 0)).expect("ok");
        // At the moment of measurement the liquid balance is at 97 which is the value returned.
        // However returning the value itself costs additional balance which results in final
        // liquid balance being 79.
        assert_eq!(outcome.liquid_balance, 79);
        match outcome.return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(decode_u64(&output_data), 97),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_get_storage_usage() {
        let input_data = [0u8; 0];
        let outcome =
            run(b"get_storage_usage", &input_data, &[], &runtime_context(0, 100, 10)).expect("ok");

        // The storage usage is not changed by this function call.
        assert_eq!(outcome.storage_usage, 10);

        match outcome.return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(decode_u64(&output_data), 10),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_storage_usage_changed() {
        let input_data = [0u8; 0];
        let outcome = run(
            b"run_test_with_storage_change",
            &input_data,
            &[],
            &runtime_context(0, 1_000_000_000, 10),
        )
        .expect("ok");

        // We inserted three entries 15 (as defined in the contract) + 4 (i32) bytes each.
        // Then we removed one entry, and replaced another with 15 + 8 (u64) bytes.
        // 52 = 10 (was before) + 15 + 4 + 15 + 8.
        assert_eq!(outcome.storage_usage, 52);
    }

    #[test]
    fn test_hello_name() {
        let input_data = b"{\"name\": \"Alice\"}";

        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../../tests/hello.wasm");
        let outcome = run_with_filename(
            b"hello",
            input_data,
            &[],
            &runtime_context(0, 1_000_000_000, 0),
            path.to_str().unwrap(),
        )
        .expect("ok");

        match outcome.return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(&output_data, b"\"hello Alice\""),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_storage_usage() {
        let input_data = b"{\"max_storage\":\"1024\"}";

        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../../tests/hello.wasm");
        let outcome = run_with_filename(
            b"limited_storage",
            input_data,
            &[],
            &runtime_context(0, 1_000_000_000, 0),
            path.to_str().unwrap(),
        )
        .expect("ok");

        println!("{:?}", outcome.storage_usage);
        println!("{:?}", outcome.logs);

        match outcome.return_data {
            Ok(ReturnData::Value(output_data)) => {
                // 1024 bytes is 207 elements.
                assert_eq!(&output_data, b"\"207\"")
            }
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_originator() {
        let input_data = [0u8; 0];

        let return_data =
            run(b"get_originator_id", &input_data, &[], &runtime_context(0, 1_000_000_000, 0))
                .map(|outcome| outcome.return_data)
                .expect("ok");

        match return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(&output_data, b"alice.near"),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_random_32() {
        let input_data = [0u8; 0];

        let mut output_data = Vec::new();

        for _ in 0..2 {
            let return_data = run(b"get_random_32", &input_data, &[], &runtime_context(0, 100, 0))
                .map(|outcome| outcome.return_data)
                .expect("ok");

            output_data.push(match return_data {
                Ok(ReturnData::Value(output_data)) => output_data,
                _ => panic!("Expected returned value"),
            });
        }

        assert_ne!(&output_data[0], &encode_i32(0));
        assert_eq!(&output_data[0], &output_data[1]);
    }

    #[test]
    fn test_random_buf() {
        let input_data = [80u8, 0, 0, 0];

        let mut output_data = Vec::new();

        for _ in 0..2 {
            let return_data =
                run(b"get_random_buf", &input_data, &[], &runtime_context(0, 1_000_000_000, 0))
                    .map(|outcome| outcome.return_data)
                    .expect("ok");

            let data = match return_data {
                Ok(ReturnData::Value(output_data)) => output_data,
                _ => panic!("Expected returned value"),
            };
            assert_eq!(data.len(), 80);

            output_data.push(data);
        }

        assert_ne!(&output_data[0][..4], &encode_i32(0));
        assert_eq!(&output_data[0], &output_data[1]);
    }

    #[test]
    fn test_hash() {
        let input_data = b"testing_hashing_this_slice";

        let return_data =
            run(b"hash_given_input", input_data, &[], &runtime_context(0, 1_000_000_000, 0))
                .map(|outcome| outcome.return_data)
                .expect("ok");

        let output_data = match return_data {
            Ok(ReturnData::Value(output_data)) => output_data,
            _ => panic!("Expected returned value"),
        };

        let expected_result: Vec<u8> = hash(input_data).into();

        assert_eq!(&output_data, &expected_result);
    }

    #[test]
    fn test_hash32() {
        let input_data = b"testing_hashing_this_slice";

        let return_data =
            run(b"hash32_given_input", input_data, &[], &runtime_context(0, 1_000_000_000, 0))
                .map(|outcome| outcome.return_data)
                .expect("ok");

        let output_data = match return_data {
            Ok(ReturnData::Value(output_data)) => output_data,
            _ => panic!("Expected returned value"),
        };

        let input_data_hash: Vec<u8> = hash(input_data).into();
        let mut expected_result = input_data_hash[..4].to_vec();
        expected_result.reverse();

        assert_eq!(&output_data, &expected_result);
    }

    #[test]
    fn test_get_block_index() {
        let input_data = [0u8; 0];

        let outcome =
            run(b"get_block_index", &input_data, &[], &runtime_context(0, 100, 0)).expect("ok");

        match outcome.return_data {
            Ok(ReturnData::Value(output_data)) => assert_eq!(decode_u64(&output_data), 123),
            _ => assert!(false, "Expected returned value"),
        };
    }

    #[test]
    fn test_debug() {
        let input_data = [0u8; 0];

        let outcome =
            run(b"log_something", &input_data, &[], &runtime_context(0, 100, 0)).expect("ok");

        assert_eq!(outcome.logs, vec!["LOG: hello".to_string(),]);
    }

    #[test]
    fn test_mock_check_ethash() {
        let input_data = [0u8; 0];
        let outcome =
            run(b"check_ethash_naive", &input_data, &[], &runtime_context(0, 100, 0)).expect("ok");
        println!("{:?}", outcome);

        let output_data = match outcome.return_data {
            Ok(ReturnData::Value(output_data)) => output_data,
            _ => panic!("Expected returned value"),
        };
        assert_eq!(output_data, encode_i32(0));
    }
}
