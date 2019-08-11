#![no_std]

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

use core::panic::PanicInfo;

#[panic_handler]
fn panic_handler(_info: &PanicInfo) -> ! {
    loop {}
}

use core::mem::size_of;

#[allow(unused)]
extern "C" {
    // #############
    // # Registers #
    // #############
    fn read_register(register_id: u64, ptr: u64);
    fn register_len(register_id: u64) -> u64;
    // ###############
    // # Context API #
    // ###############
    fn current_account_id(register_id: u64);
    fn signer_account_id(register_id: u64);
    fn signer_account_pk(register_id: u64);
    fn predecessor_account_id(register_id: u64);
    fn input(register_id: u64);
    fn block_index() -> u64;
    fn storage_usage() -> u64;
    // #################
    // # Economics API #
    // #################
    fn account_balance(balance_ptr: u64);
    fn attached_deposit(balance_ptr: u64);
    fn prepaid_gas() -> u64;
    fn used_gas() -> u64;
    // ############
    // # Math API #
    // ############
    fn random_seed(register_id: u64);
    fn sha256(value_len: u64, value_ptr: u64, register_id: u64);
    // #####################
    // # Miscellaneous API #
    // #####################
    fn value_return(value_len: u64, value_ptr: u64);
    fn panic();
    fn log_utf8(len: u64, ptr: u64);
    fn log_utf16(len: u64, ptr: u64);
    fn abort(msg_ptr: u32, filename_ptr: u32, line: u32, col: u32);
    // ################
    // # Promises API #
    // ################
    fn promise_create(
        account_id_len: u64,
        account_id_ptr: u64,
        method_name_len: u64,
        method_name_ptr: u64,
        arguments_len: u64,
        arguments_ptr: u64,
        amount_ptr: u64,
        gas: u64,
    ) -> u64;
    fn promise_then(
        promise_index: u64,
        account_id_len: u64,
        account_id_ptr: u64,
        method_name_len: u64,
        method_name_ptr: u64,
        arguments_len: u64,
        arguments_ptr: u64,
        amount_ptr: u64,
        gas: u64,
    ) -> u64;
    fn promise_and(promise_idx_ptr: u64, promise_idx_count: u64) -> u64;
    fn promise_results_count() -> u64;
    fn promise_result(result_idx: u64, register_id: u64) -> u64;
    fn promise_return(promise_id: u64);
    // ###############
    // # Storage API #
    // ###############
    fn storage_write(
        key_len: u64,
        key_ptr: u64,
        value_len: u64,
        value_ptr: u64,
        register_id: u64,
    ) -> u64;
    fn storage_read(key_len: u64, key_ptr: u64, register_id: u64) -> u64;
    fn storage_remove(key_len: u64, key_ptr: u64, register_id: u64) -> u64;
    fn storage_has_key(key_len: u64, key_ptr: u64) -> u64;
    fn storage_iter_prefix(prefix_len: u64, prefix_ptr: u64) -> u64;
    fn storage_iter_range(start_len: u64, start_ptr: u64, end_len: u64, end_ptr: u64) -> u64;
    fn storage_iter_next(iterator_id: u64, key_register_id: u64, value_register_id: u64) -> u64;
}

#[no_mangle]
pub fn write_key_value() {
    unsafe {
        input(0);
        if register_len(0) != 2 * size_of::<u64>() as u64 {
            panic()
        }
        let data = [0u8; 2 * size_of::<u64>()];
        read_register(0, data.as_ptr() as u64);

        let key = &data[0..size_of::<u64>()];
        let value = &data[size_of::<u64>()..];
        let result = storage_write(
            key.len() as u64,
            key.as_ptr() as u64,
            value.len() as u64,
            value.as_ptr() as u64,
            1,
        );
        value_return(size_of::<u64>() as u64, &result as *const u64 as u64);
    }
}

#[no_mangle]
pub fn read_value() {
    unsafe {
        input(0);
        if register_len(0) != size_of::<u64>() as u64 {
            panic()
        }
        let key = [0u8; size_of::<u64>()];
        read_register(0, key.as_ptr() as u64);
        let result = storage_read(key.len() as u64, key.as_ptr() as u64, 1);
        if result == 1 {
            let value = [0u8; size_of::<u64>()];
            read_register(1, value.as_ptr() as u64);
            value_return(value.len() as u64, &value as *const u8 as u64);
        }
    }
}

#[no_mangle]
pub fn log_something() {
    unsafe {
        let data = b"hello";
        log_utf8(data.len() as u64, data.as_ptr() as _);
    }
}

#[no_mangle]
pub fn run_test() {
    unsafe {
        let value: [u8; 4] = 10i32.to_le_bytes();
        value_return(value.len() as u64, value.as_ptr() as _);
    }
}

#[no_mangle]
pub fn sum_with_input() {
    unsafe {
        input(0);
        if register_len(0) != 2 * size_of::<u64>() as u64 {
            panic()
        }
        let data = [0u8; 2 * size_of::<u64>()];
        read_register(0, data.as_ptr() as u64);

        let mut key = [0u8; size_of::<u64>()];
        let mut value = [0u8; size_of::<u64>()];
        key.copy_from_slice(&data[..size_of::<u64>()]);
        value.copy_from_slice(&data[size_of::<u64>()..]);
        let key = core::u64::from_le_bytes(key);
        let value = core::u64::from_le_bytes(value);
        let result = key + value;
        value_return(size_of::<u64>() as u64, &result as *const u64 as u64);
    }
}
