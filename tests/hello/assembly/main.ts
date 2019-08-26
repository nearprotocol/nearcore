//@nearfile
import { near, context, storage, logging, base58, base64, PersistentMap, PersistentVector, PersistentDeque, PersistentTopN, ContractPromise, math } from "near-runtime-ts";
import { u128 } from "bignum";
import { MyCallbackResult, MyContractPromiseResult }  from "./model";

export function hello(name: string): string {

  return "hello " + name;
}

/*
export function setKeyValue(key: string, value: string): void {
  storage.set<string>(key!, value!);
}

export function getValueByKey(key: string): string {
  return storage.get<string>(key!);
}

export function setValue(value: string): string {
  storage.set<string>("name", value!);
  return value!;
}

export function getValue(): string | null {
  return storage.get<string>("name");
} */

export function getAllKeys(): string[] {
  let keys = storage.keys("n");
  assert(keys.length == 1);
  assert(keys[0] == "name");
  return keys;
}

export function benchmark(): string[] {
  let i = 0;
  while (i < 10) {
    storage.set<string>(i.toString(), "123123");
    i += 1;
  }
  return storage.keys("");
}


export function limited_storage(max_storage: u64): string {
  let i = 0;
  while (context.storageUsage <= max_storage) {
    i += 1;
    storage.set<string>(i.toString(), i.toString());
  }
  if (context.storageUsage > max_storage) {
    storage.delete(i.toString());
  }
  return i.toString()
}

export function benchmark_sum_n(n: i32): string {
  let i = 0;
  let sum: u64 = 0;
  while (i < n) {
    sum += i;
    i += 1;
  }
  return sum.toString()
}


export function generateLogs(): void {
  storage.set<string>("item", "value");
  logging.log("log1");
  logging.log("log2");
}

export function returnHiWithLogs(): string {
  logging.log("loooog1");
  logging.log("loooog2");
  return "Hi"
}

export function triggerAssert(): void {
  logging.log("log before assert");
  assert(false, "expected to fail");
}

export function testSetRemove(value: string): void {
  storage.set<string>("test", value);
  storage.delete("test");
  assert(storage.get<string>("test") == null, "Item must be empty");
}

function buildString(n: i32): string {
  assert(n >= 0);
  let result = "";
  for (let i = 20; i >= 0; --i) {
    result = result + result;
    if ((n >> i) & 1) {
      result += "a";
    }
  }
  return result;
}

export function insertStrings(from: i32, to: i32): void {
  let str = buildString(to);
  for (let i = from; i < to; i++) {
    storage.set<string>(str.substr(to - i) + "b", "x");
  }
}

export function deleteStrings(from: i32, to: i32): void {
  let str = buildString(to);
  for (let i = to - 1; i >= from; i--) {
    storage.delete(str.substr(to - i) + "b");
  }
}

export function recurse(n: i32): i32 {
  if (n <= 0) {
    return n;
  }
  return recurse(n - 1) + 1;
}

// For testing promises

/*
export function callPromise(args: PromiseArgs): void {
  let inputArgs: InputPromiseArgs = { args: args.args };
  let balance = args.balance as u64;
  let promise = ContractPromise.create(
      args.receiver,
      args.methodName,
      inputArgs.encode(),
      new u128(args.balance));
  if (args.callback) {
    inputArgs.args = args.callbackArgs;
    let callbackBalance = args.callbackBalance as u64;
    promise = promise.then(
        args.callback,
        inputArgs.encode(),
        new u128(callbackBalance));
  }
  promise.returnAsResult();
}

export function callbackWithName(args: PromiseArgs): MyCallbackResult {
  let contractResults = ContractPromise.getResults();
  let allRes = Array.create<MyContractPromiseResult>(contractResults.length);
  for (let i = 0; i < contractResults.length; ++i) {
    allRes[i] = new MyContractPromiseResult();
    allRes[i].ok = contractResults[i].success;
    if (allRes[i].ok && contractResults[i].buffer != null && contractResults[i].buffer.length > 0) {
      allRes[i].r = MyCallbackResult.decode(contractResults[i].buffer);
    }
  }
  let result: MyCallbackResult = {
    rs: allRes,
    n: context.contractName,
  };
  let bytes = result.encode();
  storage.setBytes("lastResult", bytes);
  return result;
}

export function getLastResult(): MyCallbackResult | null {
  return storage.get<MyCallbackResult>("lastResult");
  //MyCallbackResult.decode(storage.getBytes("lastResult"));
} */
