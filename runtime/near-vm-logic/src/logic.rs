use crate::config::Config;
use crate::context::RuntimeContext;
use crate::dependencies::{External, MemoryLike};
use crate::errors::HostError;
use crate::rand_iter::RandIterator;
use crate::types::{
    AccountId, Balance, Gas, PromiseIndex, PromiseResult, ReturnData, StorageUsage,
};
use std::collections::HashMap;
use std::mem::size_of;

type Result<T> = ::std::result::Result<T, HostError>;

pub struct Runtime<'a, E: 'static, M: 'static> {
    /// Provides access to the components outside the Wasm runtime for operations on the trie and
    /// receipts creation.
    ext: &'a mut E,
    /// Part of Context API and Economics API that was extracted from the receipt.
    context: RuntimeContext<'a>,
    /// Parameters of Wasm and economic parameters.
    config: Config,
    /// If this method execution is invoked directly as a callback by one or more contract calls the
    /// results of the methods that made the callback are stored in this collection.
    promise_results: &'a [PromiseResult],
    /// Pointer to the guest memory.
    memory: &'a mut M,

    /// Keeping track of the current account balance, which can decrease when we create promises
    /// and attach balance to them.
    current_account_balance: Balance,
    /// The amount of gas that was irreversibly used for contract execution.
    burnt_gas: Gas,
    /// `burnt_gas` + gas that was attached to the promises.
    used_gas: Gas,
    /// What method returns.
    return_data: ReturnData,
    /// Logs written by the runtime.
    logs: Vec<String>,
    /// Registers can be used by the guest to store blobs of data without moving them across
    /// host-guest boundary.
    registers: HashMap<u64, Vec<u8>>,
    /// Endless random iterator is used to generate random bytes.
    rand_iter: RandIterator,
}

impl<'a, E: 'static, M: 'static> Runtime<'a, E, M>
where
    E: External,
    M: MemoryLike,
{
    pub fn new(
        ext: &'a mut E,
        context: RuntimeContext<'a>,
        config: Config,
        promise_results: &'a [PromiseResult],
        memory: &'a mut M,
    ) -> Self {
        let rand_iter = RandIterator::new(&context.random_seed);
        let current_account_balance = context.account_balance;
        Self {
            ext,
            context,
            config,
            promise_results,
            memory,
            current_account_balance,
            burnt_gas: 0,
            used_gas: 0,
            return_data: ReturnData::None,
            logs: vec![],
            registers: HashMap::new(),
            rand_iter,
        }
    }

    // ###########################
    // # Memory helper functions #
    // ###########################

    fn try_fit_mem(memory: &M, offset: u64, len: u64) -> Result<()> {
        if memory.fits_memory(offset, len) {
            Ok(())
        } else {
            Err(HostError::MemoryAccessViolation)
        }
    }

    fn memory_get(memory: &M, offset: u64, len: u64) -> Result<Vec<u8>> {
        Self::try_fit_mem(memory, offset, len)?;
        Ok(memory.read_memory(offset, len))
    }

    fn memory_set(memory: &mut M, offset: u64, buf: &[u8]) -> Result<()> {
        Self::try_fit_mem(memory, offset, buf.len() as _)?;
        Ok(memory.write_memory(offset, buf))
    }

    /// Writes `u128` to Wasm memory.
    /// Wasm defaults to little endian byte ordering, so we also use it here.
    /// See: https://github.com/WebAssembly/design/blob/master/Portability.md
    fn memory_set_u128(memory: &mut M, offset: u64, value: u128) -> Result<()> {
        let data: [u8; size_of::<u128>()] = value.to_le_bytes();
        Self::memory_set(memory, offset, &data)
    }

    /// Get `u128` from Wasm memory.
    fn memory_get_u128(memory: &M, offset: u64) -> Result<u128> {
        let buf = Self::memory_get(memory, offset, size_of::<u128>() as _)?;
        let mut array = [0u8; size_of::<u128>()];
        // Enforce panic if wrong number of bytes read.
        array.copy_from_slice(&buf[0..size_of::<u128>()]);
        Ok(u128::from_le_bytes(array))
    }

    /// Reads an array of `u64` elements.
    fn memory_get_array_u64(memory: &M, offset: u64, num_elements: u64) -> Result<Vec<u64>> {
        let data = Self::memory_get(memory, offset, num_elements * size_of::<u64>() as u64)?;
        Ok(data
            .chunks(size_of::<u64>())
            .map(|buf| {
                assert_eq!(buf.len(), size_of::<u64>());
                let mut array = [0u8; size_of::<u64>()];
                array.copy_from_slice(buf);
                u64::from_le_bytes(array)
            })
            .collect())
    }

    // #################
    // # Registers API #
    // #################

    /// Writes the entire content from the register `register_id` into the memory of the guest starting with `ptr`.
    ///
    /// # Arguments
    ///
    /// * `register_id` -- a register id from where to read the data;
    /// * `ptr` -- location on guest memory where to copy the data.
    ///
    /// # Panics
    ///
    /// * If the content extends outside the memory allocated to the guest. In Wasmer, it returns `MemoryAccessViolation` error message;
    /// * If `register_id` is pointing to unused register returns `InvalidRegisterId` error message.
    ///
    /// # Undefined Behavior
    ///
    /// If the content of register extends outside the preallocated memory on the host side, or the pointer points to a
    /// wrong location this function will overwrite memory that it is not supposed to overwrite causing an undefined behavior.
    fn read_register(&mut self, register_id: u64, ptr: u64) -> Result<()> {
        let Self { registers, memory, .. } = self;
        let register = registers.get(&register_id).ok_or(HostError::InvalidRegisterId)?;
        Self::memory_set(memory, ptr, register)
    }

    /// Returns the size of the blob stored in the given register.
    /// * If register is used, then returns the size, which can potentially be zero;
    /// * If register is not used, returns `u64::MAX`
    ///
    /// # Arguments
    ///
    /// * `register_id` -- a register id from where to read the data;
    fn register_len(&mut self, register_id: u64) -> Result<u64> {
        Ok(self.registers.get(&register_id).map(|r| r.len() as _).unwrap_or(std::u64::MAX))
    }

    /// Copies `data` into register. If register is unused will initialize it. If register has
    /// larger capacity than needed for `data` will not re-allocate it. The register will lose
    /// the pre-existing data if any.
    ///
    /// # Arguments
    ///
    /// * `register_id` -- a register into which to write the data;
    /// * `data` -- data to be copied into register.
    fn write_register(&mut self, register_id: u64, data: &[u8]) -> Result<()> {
        if data.len() as u64 > self.config.max_register_size
            || self.registers.len() as u64 == self.config.max_number_registers
        {
            return Err(HostError::MemoryAccessViolation);
        }
        let register = self.registers.entry(register_id).or_insert_with(Vec::new);
        register.clear();
        register.reserve(data.len());
        register.extend_from_slice(data);

        // Calculate the new memory usage before copying.
        let usage: usize = self
            .registers
            .values()
            .map(|v| size_of::<u64>() + v.capacity() * size_of::<u8>())
            .sum();
        if usage > self.config.registers_memory_limit as _ {
            Err(HostError::MemoryAccessViolation)
        } else {
            Ok(())
        }
    }

    // ###############
    // # Context API #
    // ###############

    /// Saves the account id of the current contract that we execute into the register.
    ///
    /// # Panics
    ///
    /// If the registers exceed the memory limit panics with `MemoryAccessViolation`.
    fn current_account_id(&mut self, register_id: u64) -> Result<()> {
        let data = self.context.current_account_id.as_bytes();
        self.write_register(register_id, data)
    }

    /// All contract calls are a result of some transaction that was signed by some account using
    /// some access key and submitted into a memory pool (either through the wallet using RPC or by
    /// a node itself). This function returns the id of that account. Saves the bytes of the signer
    /// account id into the register.
    ///
    /// # Panics
    ///
    /// If the registers exceed the memory limit panics with `MemoryAccessViolation`.
    fn signer_account_id(&mut self, register_id: u64) -> Result<()> {
        let data = self.context.signer_account_id.as_bytes();
        self.write_register(register_id, data)
    }

    /// Saves the public key fo the access key that was used by the signer into the register. In
    /// rare situations smart contract might want to know the exact access key that was used to send
    /// the original transaction, e.g. to increase the allowance or manipulate with the public key.
    ///
    /// # Panics
    ///
    /// If the registers exceed the memory limit panics with `MemoryAccessViolation`.
    fn signer_account_pk(&mut self, register_id: u64) -> Result<()> {
        let data = self.context.signer_account_pk.as_ref();
        self.write_register(register_id, data)
    }

    /// All contract calls are a result of a receipt, this receipt might be created by a transaction
    /// that does function invocation on the contract or another contract as a result of
    /// cross-contract call. Saves the bytes of the predecessor account id into the register.
    ///
    /// # Panics
    /// If the registers exceed the memory limit panics with `MemoryAccessViolation`.
    /// TODO: Implement once https://github.com/nearprotocol/NEPs/pull/8 is complete.
    fn predecessor_account_id(&mut self, _register_id: u64) -> Result<()> {
        unimplemented!()
    }

    /// Reads input to the contract call into the register. Input is expected to be in JSON-format.
    /// f input is provided saves the bytes (potentially zero) of input into register. If input is
    /// not provided makes the register "not used", i.e. `register_len` now returns `u64::MAX`.
    fn input(&mut self, register_id: u64) -> Result<()> {
        self.write_register(register_id, self.context.input)
    }

    /// Returns the current block index.
    fn block_index(&self) -> Result<u64> {
        Ok(self.context.block_index)
    }

    /// Returns the number of bytes used by the contract if it was saved to the trie as of the
    /// invocation. This includes:
    /// * The data written with storage_* functions during current and previous execution;
    /// * The bytes needed to store the account protobuf and the access keys of the given account.
    ///
    /// TODO: Include the storage of the account proto and all access keys. Implement once
    /// https://github.com/nearprotocol/NEPs/pull/8 is complete.
    fn storage_usage(&self) -> Result<StorageUsage> {
        Ok(self.ext.storage_usage())
    }

    // #################
    // # Economics API #
    // #################

    /// The balance attached to the given account. This includes the attached_deposit that was
    /// attached to the transaction
    /// TODO: Make sure we actually add the deposit before running contract.
    fn account_balance(&mut self, balance_ptr: u64) -> Result<()> {
        Self::memory_set(&mut self.memory, balance_ptr, &self.context.account_balance.to_le_bytes())
    }

    /// The balance that was attached to the call that will be immediately deposited before the
    /// contract execution starts
    fn attached_deposit(&mut self, balance_ptr: u64) -> Result<()> {
        Self::memory_set(
            &mut self.memory,
            balance_ptr,
            &self.context.attached_deposit.to_le_bytes(),
        )
    }

    /// The amount of gas attached to the call that can be used to pay for the gas fees.
    /// TODO: Implement once https://github.com/nearprotocol/NEPs/pull/8 is complete.
    fn prepaid_gas(&mut self) -> Result<u64> {
        unimplemented!()
    }

    /// The gas that was already burnt during the contract execution (cannot exceed `prepaid_gas`)
    /// TODO: Implement once https://github.com/nearprotocol/NEPs/pull/8 is complete.
    fn used_gas(&mut self) -> Result<u64> {
        unimplemented!()
    }

    // ############
    // # Math API #
    // ############

    /// Writes random bytes in the given register.
    ///
    /// # Panics
    /// If the size of the registers exceed the set limit `MemoryAccessViolation`.
    fn random_buf(&mut self, len: u64, register_id: u64) -> Result<()> {
        let mut buf = vec![];
        for _ in 0..len {
            buf.push(self.rand_iter.next().unwrap());
        }
        self.write_register(register_id, &buf)
    }

    /// Returns a random `u64` variable.
    fn random_u64(&mut self) -> Result<u64> {
        let mut buf = [0u8; size_of::<u64>()];
        for i in 0..size_of::<u64>() {
            buf[i] = self.rand_iter.next().unwrap();
        }
        Ok(u64::from_le_bytes(buf))
    }

    /// Hashes the random sequence of bytes using sha256 and returns it into `register_id`.
    ///
    /// # Panics
    /// If `value_len + value_ptr` points outside the memory or the registers use more memory than
    /// the limit with `MemoryAccessViolation`.
    fn sha256(&mut self, value_len: u64, value_ptr: u64, register_id: u64) -> Result<()> {
        let value = Self::memory_get(&self.memory, value_ptr, value_len)?;
        let value_hash = exonum_sodiumoxide::crypto::hash::sha256::hash(&value);
        self.write_register(register_id, value_hash.as_ref())
    }

    // ################
    // # Promises API #
    // ################

    /// Creates a promise that will execute a method on account with given arguments and attaches
    /// the given amount and gas. `amount_ptr` point to slices of bytes representing `u128`.
    ///
    /// # Panics
    ///
    /// If `account_id_len + account_id_ptr` or `method_name_len + method_name_ptr` or
    /// `arguments_len + arguments_ptr` or `amount_ptr + 16` points outside the memory of the guest
    /// or host, with `MemoryAccessViolation`.
    ///
    /// # Returns
    ///
    /// Index of the new promise that uniquely identifies it within the current execution of the
    /// method.
    fn promise_create(
        &mut self,
        account_id_len: u64,
        account_id_ptr: u64,
        method_name_len: u64,
        method_name_ptr: u64,
        arguments_len: u64,
        arguments_ptr: u64,
        amount_ptr: u64,
        gas: Gas,
    ) -> Result<u64> {
        let amount = Self::memory_get_u128(&self.memory, amount_ptr)?;
        let account_id = self.read_and_parse_account_id(account_id_ptr, account_id_len)?;
        let method_name = Self::memory_get(&self.memory, method_name_ptr, method_name_len)?;

        if let Some(b'_') = method_name.get(0) {
            return Err(HostError::PrivateMethod);
        }

        let arguments = Self::memory_get(&self.memory, arguments_ptr, arguments_len)?;
        self.attach_gas_to_promise(gas)?;
        self.ext
            .promise_create(account_id, method_name, arguments, amount, gas)
            .map_err(|err| err.into())
    }

    /// Attaches the callback that is executed after promise pointed by `promise_idx` is complete.
    ///
    /// # Panics
    ///
    /// * If `promise_idx` does not correspond to an existing promise panics with
    ///   `InvalidPromiseIndex`;
    /// * If `account_id_len + account_id_ptr` or `method_name_len + method_name_ptr` or
    ///   `arguments_len + arguments_ptr` or `amount_ptr + 16` points outside the memory of the
    ///   guest or host, with `MemoryAccessViolation`.
    ///
    /// # Returns
    ///
    /// Index of the new promise that uniquely identifies it within the current execution of the
    /// method.
    fn promise_then(
        &mut self,
        promise_idx: u64,
        account_id_len: u64,
        account_id_ptr: u64,
        method_name_len: u64,
        method_name_ptr: u64,
        arguments_len: u64,
        arguments_ptr: u64,
        amount_ptr: u64,
        gas: u64,
    ) -> Result<u64> {
        let account_id = self.read_and_parse_account_id(account_id_ptr, account_id_len)?;
        let amount = Self::memory_get_u128(&self.memory, amount_ptr)?;
        let method_name = Self::memory_get(&self.memory, method_name_ptr, method_name_len)?;
        if method_name.is_empty() {
            return Err(HostError::EmptyMethodName);
        }
        let arguments = Self::memory_get(&self.memory, arguments_ptr, arguments_len)?;
        self.ext
            .promise_then(promise_idx, account_id, method_name, arguments, amount, gas)
            .map_err(|err| err.into())
    }

    /// Creates a new promise which completes when time all promises passed as arguments complete.
    /// Cannot be used with registers. `promise_idx_ptr` points to an array of `u64` elements, with
    /// `promise_idx_count` denoting the number of elements. The array contains indices of promises
    /// that need to be waited on jointly.
    ///
    /// # Panics
    ///
    /// * If `promise_ids_ptr + 8 * promise_idx_count` extend outside the guest memory with
    ///   `MemoryAccessViolation`;
    /// * If any of the promises in the array do not correspond to existing promises panics with
    ///   `InvalidPromiseIndex`.
    ///
    /// # Returns
    ///
    /// Index of the new promise that uniquely identifies it within the current execution of the
    /// method.
    fn promise_and(
        &mut self,
        promise_idx_ptr: u64,
        promise_idx_count: u64,
    ) -> Result<PromiseIndex> {
        let promise_ids =
            Self::memory_get_array_u64(&self.memory, promise_idx_ptr, promise_idx_count)?;
        self.ext.promise_and(&promise_ids).map_err(|err| err.into())
    }

    /// If the current function is invoked by a callback we can access the execution results of the
    /// promises that caused the callback. This function returns the number of complete and
    /// incomplete callbacks.
    ///
    /// Note, we are only going to have incomplete callbacks once we have promise_or combinator.
    ///
    ///
    /// * If there is only one callback returns `1`;
    /// * If there are multiple callbacks (e.g. created through `promise_and`) returns their number;
    /// * If the function was called not through the callback returns `0`.
    fn promise_results_count(&self) -> Result<u64> {
        Ok(self.promise_results.len() as _)
    }

    /// If the current function is invoked by a callback we can access the execution results of the
    /// promises that caused the callback. This function returns the result in blob format and
    /// places it into the register.
    ///
    /// * If promise result is complete and successful copies its blob into the register;
    /// * If promise result is complete and failed or incomplete keeps register unused;
    ///
    /// # Returns
    ///
    /// * If promise result is not complete returns `0`;
    /// * If promise result is complete and successful returns `1`;
    /// * If promise result is complete and failed returns `2`.
    ///
    /// # Panics
    ///
    /// * If `result_idx` does not correspond to an existing result panics with
    ///   `InvalidResultIndex`;
    /// * If copying the blob exhausts the memory limit it panics with `MemoryAccessViolation`.
    fn promise_result(&mut self, result_idx: u64, register_id: u64) -> Result<u64> {
        match self
            .promise_results
            .get(result_idx as usize)
            .ok_or(HostError::InvalidPromiseResultIndex)?
        {
            PromiseResult::NotReady => Ok(0),
            PromiseResult::Successful(data) => {
                self.write_register(register_id, data)?;
                Ok(1)
            }
            PromiseResult::Failed => Ok(2),
        }
    }

    /// When promise `promise_idx` finishes executing its result is considered to be the result of
    /// the current function.
    ///
    /// # Panics
    ///
    /// If `promise_idx` does not correspond to an existing promise panics with
    /// `InvalidPromiseIndex`.
    fn promise_return(&mut self, promise_idx: u64) -> Result<()> {
        self.return_data = ReturnData::Promise(promise_idx);
        Ok(())
    }

    // #####################
    // # Miscellaneous API #
    // #####################
    /// Sets the blob of data as the return value of the contract.
    ///
    /// # Panics
    /// If `value_len + value_ptr` exceeds the memory container or points to an unused register it
    /// panics with `MemoryAccessViolation`.
    fn value_return(&mut self, value_len: u64, value_ptr: u64) -> Result<()> {
        let return_val = Self::memory_get(&self.memory, value_ptr, value_len)?;
        self.return_data = ReturnData::Value(return_val);
        Ok(())
    }

    /// Terminates the execution of the program with panic `GuestPanic`.
    fn panic(&self) -> Result<()> {
        Err(HostError::GuestPanic)
    }

    /// Logs the UTF-8 encoded string.
    /// If `len == u64::MAX` then treats the string as null-terminated with character `'\0'`.
    ///
    /// # Panics
    ///
    /// * If string extends outside the memory of the guest with `MemoryAccessViolation`;
    /// * If string is not UTF-8 panics with `BadUtf8`.
    fn log_utf8(&mut self, len: u64, ptr: u64) -> Result<()> {
        let mut buf;
        if len != std::u64::MAX {
            if len > self.config.max_log_len {
                return Err(HostError::BadUTF8);
            }
            buf = Self::memory_get(&self.memory, ptr, len)?;
        } else {
            buf = vec![];
            for i in 0..=self.config.max_log_len {
                if i == self.config.max_log_len {
                    return Err(HostError::BadUTF8);
                }
                Self::try_fit_mem(&self.memory, ptr, i)?;
                let el = self.memory.read_memory_u8(ptr + i);
                if el == 0 {
                    break;
                }
                buf.push(el);
            }
        }
        let str = String::from_utf8(buf).map_err(|_| HostError::BadUTF8)?;
        let message = format!("LOG: {}", str);
        self.logs.push(message);
        Ok(())
    }

    /// Helper function to read UTF-16 from guest memory.
    fn get_utf16(&mut self, len: u64, ptr: u64) -> Result<String> {
        let mut buf;
        if len != std::u64::MAX {
            if len > self.config.max_log_len || len % 2 != 0 {
                return Err(HostError::BadUTF16);
            }
            buf = Self::memory_get(&self.memory, ptr, len)?;
        } else {
            buf = vec![];
            let mut prev_nul = false;
            for i in 0..=self.config.max_log_len {
                if i == self.config.max_log_len {
                    return Err(HostError::BadUTF16);
                }
                Self::try_fit_mem(&self.memory, ptr, i)?;
                let el: u8 = self.memory.read_memory_u8(ptr + i);
                let curr_nul = el == 0;
                if prev_nul && curr_nul {
                    if i % 2 == 1 {
                        break;
                    } else {
                        return Err(HostError::BadUTF16);
                    }
                }
                buf.push(el);
                prev_nul = curr_nul;
            }
        }
        let buf: Vec<u16> = (0..len)
            .step_by(2)
            .map(|i| u16::from_le_bytes([buf[i as usize], buf[i as usize + 1]]))
            .collect();
        String::from_utf16(&buf).map_err(|_| HostError::BadUTF16)
    }

    /// Logs the UTF-16 encoded string. If `len == u64::MAX` then treats the string as
    /// null-terminated with two-byte sequence of `0x00 0x00`.
    ///
    /// # Panics
    ///
    /// * If string extends outside the memory of the guest with `MemoryAccessViolation`;
    /// * If string is not UTF-16 panics with `BadUtf16`.
    fn log_utf16(&mut self, ptr: u64, len: u64) -> Result<()> {
        let str = self.get_utf16(ptr, len)?;
        let message = format!("LOG: {}", str);
        self.logs.push(message);
        Ok(())
    }

    /// Special import kept for compatibility with AssemblyScript contracts. Not called by smart
    /// contracts directly, but instead called by the code generated by AssemblyScript.
    fn abort(&mut self, msg_ptr: u32, filename_ptr: u32, line: u32, col: u32) -> Result<()> {
        let msg = self.get_utf16(msg_ptr as _, std::u64::MAX)?;
        let filename = self.get_utf16(filename_ptr as _, std::u64::MAX)?;

        let message =
            format!("ABORT: {:?} filename: {:?} line: {:?} col: {:?}", msg, filename, line, col);
        self.logs.push(message);

        Err(HostError::GuestPanic)
    }

    /// Reads account id from the given location in memory.
    ///
    /// # Errors
    ///
    /// * If account is not UTF-8 encoded then returns `BadUtf8`;
    fn read_and_parse_account_id(&self, ptr: u64, len: u64) -> Result<AccountId> {
        let buf = Self::memory_get(&self.memory, ptr, len)?;
        let account_id = AccountId::from_utf8(buf).map_err(|_| HostError::BadUTF8)?;
        Ok(account_id)
    }

    /// Called by gas metering injected into Wasm. Counts both towards `burnt_gas` and `used_gas`.
    ///
    /// # Errors
    ///
    /// * If passed gas amount somehow overflows internal gas counters returns `IntegerOverflow`;
    /// * If we exceed usage limit imposed on burnt gas returns `UsageLimit`;
    /// * If we exceed the `prepaid_gas` then returns `BalanceExceeded`.
    fn gas(&mut self, gas_amount: u32) -> Result<()> {
        let new_burnt_gas =
            self.burnt_gas.checked_add(gas_amount as _).ok_or(HostError::IntegerOverflow)?;
        let new_used_gas =
            self.used_gas.checked_add(gas_amount as _).ok_or(HostError::IntegerOverflow)?;
        if new_burnt_gas < self.config.max_gas_burnt && new_used_gas < self.context.prepaid_gas {
            self.burnt_gas = new_burnt_gas;
            self.used_gas = new_used_gas;
            Ok(())
        } else {
            use std::cmp::min;
            let res = if new_burnt_gas >= self.config.max_gas_burnt {
                Err(HostError::UsageLimit)
            } else if new_used_gas >= self.context.prepaid_gas {
                Err(HostError::BalanceExceeded)
            } else {
                unreachable!()
            };
            self.burnt_gas = min(new_burnt_gas, self.config.max_gas_burnt);
            self.used_gas = min(new_used_gas, self.context.prepaid_gas);
            res
        }
    }

    /// Called upon creating a promise. Counts towards `used_gas` but not `burnt_gas`.
    ///
    /// # Errors:
    ///
    /// * If passed gas amount somehow overflows internal gas counters returns `IntegerOverflow`;
    /// * If we exceed the `prepaid_gas` then returns `BalanceExceeded`.
    fn attach_gas_to_promise(&mut self, gas_amount: u64) -> Result<()> {
        let new_used_gas =
            self.used_gas.checked_add(gas_amount).ok_or(HostError::IntegerOverflow)?;
        if new_used_gas < self.context.prepaid_gas {
            self.used_gas = new_used_gas;
            Ok(())
        } else {
            let res = Err(HostError::BalanceExceeded);
            self.used_gas = self.context.prepaid_gas;
            res
        }
    }

    /// Writes key-value into storage.
    /// * If key is not in use it inserts the key-value pair and does not modify the register. Returns `0`;
    /// * If key is in use it inserts the key-value and copies the old value into the `register_id`. Returns `1`.
    ///
    /// # Panics
    ///
    /// * If `key_len + key_ptr` or `value_len + value_ptr` exceeds the memory container or points
    ///   to an unused register it panics with `MemoryAccessViolation`;
    /// * If returning the preempted value into the registers exceed the memory container it panics
    ///   with `MemoryAccessViolation`.
    fn storage_write(
        &mut self,
        key_len: u64,
        key_ptr: u64,
        value_len: u64,
        value_ptr: u64,
        register_id: u64,
    ) -> Result<u64> {
        let key = Self::memory_get(&self.memory, key_ptr, key_len)?;
        let value = Self::memory_get(&self.memory, value_ptr, value_len)?;
        let evicted = self.ext.storage_set(&key, &value)?;
        match evicted {
            Some(value) => {
                self.write_register(register_id, &value)?;
                Ok(1)
            }
            None => Ok(0),
        }
    }

    /// Reads the value stored under the given key.
    /// * If key is used copies the content of the value into the `register_id`, even if the content
    ///   is zero bytes. Returns `1`;
    /// * If key is not present then does not modify the register. Returns `0`;
    ///
    /// # Panics
    ///
    /// * If `key_len + key_ptr` exceeds the memory container or points to an unused register it
    ///   panics with `MemoryAccessViolation`;
    /// * If returning the preempted value into the registers exceed the memory container it panics
    ///   with `MemoryAccessViolation`.
    fn storage_read(&mut self, key_len: u64, key_ptr: u64, register_id: u64) -> Result<u64> {
        let key = Self::memory_get(&self.memory, key_ptr, key_len)?;
        let read = self.ext.storage_get(&key)?;
        match read {
            Some(value) => {
                self.write_register(register_id, &value)?;
                Ok(1)
            }
            None => Ok(0),
        }
    }

    /// Removes the value stored under the given key.
    /// * If key is used, removes the key-value from the trie and copies the content of the value
    ///   into the `register_id`, even if the content is zero bytes. Returns `1`;
    /// * If key is not present then does not modify the register. Returns `0`.
    ///
    /// # Panics
    ///
    /// * If `key_len + key_ptr` exceeds the memory container or points to an unused register it
    ///   panics with `MemoryAccessViolation`;
    /// * If the registers exceed the memory limit panics with `MemoryAccessViolation`;
    /// * If returning the preempted value into the registers exceed the memory container it panics
    ///   with `MemoryAccessViolation`.
    fn storage_remove(&mut self, key_len: u64, key_ptr: u64, register_id: u64) -> Result<u64> {
        let key = Self::memory_get(&self.memory, key_ptr, key_len)?;
        let removed = self.ext.storage_remove(&key)?;
        match removed {
            Some(value) => {
                self.write_register(register_id, &value)?;
                Ok(1)
            }
            None => Ok(0),
        }
    }

    /// Checks if there is a key-value pair.
    /// * If key is used returns `1`, even if the value is zero bytes;
    /// * Otherwise returns `0`.
    ///
    /// # Panics
    ///
    /// If `key_len + key_ptr` exceeds the memory container it panics with `MemoryAccessViolation`.
    fn storage_has_key(&mut self, key_len: u64, key_ptr: u64) -> Result<u64> {
        let key = Self::memory_get(&self.memory, key_ptr, key_len)?;
        let res = self.ext.storage_has_key(&key)?;
        Ok(res as u64)
    }

    /// Gets iterator for keys with given prefix
    fn storage_iter_prefix(&mut self, prefix_len: u64, prefix_ptr: u64) -> Result<u64> {
        let prefix = Self::memory_get(&self.memory, prefix_ptr, prefix_len)?;
        let storage_id = self.ext.storage_iter(&prefix)?;
        Ok(storage_id)
    }

    /// Gets iterator for the range of keys between given start and end keys
    fn storage_range(
        &mut self,
        start_len: u64,
        start_ptr: u64,
        end_len: u64,
        end_ptr: u64,
    ) -> Result<u64> {
        let start_key = Self::memory_get(&self.memory, start_ptr, start_len)?;
        let end_key = Self::memory_get(&self.memory, end_ptr, end_len)?;
        self.ext.storage_range(&start_key, &end_key).map_err(|err| err.into())
    }

    /// Advances iterator. Returns true if iteration isn't finished yet.
    fn storage_iter_next(
        &mut self,
        iterator_id: u64,
        key_register_id: u64,
        value_register_id: u64,
    ) -> Result<u64> {
        let value = self.ext.storage_iter_next(iterator_id)?;
        match value {
            Some((key, value)) => {
                self.write_register(key_register_id, &key)?;
                self.write_register(value_register_id, &value)?;
                Ok(1)
            }
            None => Ok(0),
        }
    }
}
