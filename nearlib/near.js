const NearClient = require('./nearclient');
const BrowserLocalStorageKeystore = require('./signing/browser_local_storage_key_store');
const SimpleKeyStoreSigner = require('./signing/simple_key_store_signer');
const LocalNodeConnection = require('./local_node_connection');

/*
 * This is javascript library for interacting with blockchain.
 */
class Near {
    constructor(nearClient) {
        this.nearClient = nearClient;
    }

    /**
     * Default setup for browser
     */
    static createDefaultConfig(nodeUrl = 'http://localhost:3030') {
        return new Near(new NearClient(
            new SimpleKeyStoreSigner(new BrowserLocalStorageKeystore()),
            new LocalNodeConnection(nodeUrl)
        ));
    }

    /**
     * Calls a view function. Returns the same value that the function returns.
     */
    async callViewFunction(sender, contractAccountId, methodName, args) {
        if (!args) {
            args = {};
        }
        const serializedArgs = Array.from(Buffer.from(JSON.stringify(args)));
        const response = await this.nearClient.request('call_view_function', {
            originator: sender,
            contract_account_id: contractAccountId,
            method_name: methodName,
            args: serializedArgs
        });
        const json = JSON.parse(Buffer.from(response.result).toString());
        return json.result;
    }

    /**
     * Schedules an asynchronous function call.
     */
    async scheduleFunctionCall(amount, sender, contractAccountId, methodName, args) {
        if (!args) {
            args = {};
        }
        const serializedArgs = Array.from(Buffer.from(JSON.stringify(args)));
        return await this.nearClient.submitTransaction('schedule_function_call', {
            amount: amount,
            originator: sender,
            contract_account_id: contractAccountId,
            method_name: methodName,
            args: serializedArgs
        });
    }

    /**
     * Deploys a contract.
     */
    async deployContract(senderAccountId, contractAccountId, wasmArray, publicKey) {
        return await this.nearClient.submitTransaction('deploy_contract', {
            originator: senderAccountId,
            contract_account_id: contractAccountId,
            wasm_byte_array: wasmArray,
            public_key: publicKey
        });
    }

    async getTransactionStatus (transaction_hash) {
        const transactionStatusResponse = await this.nearClient.request('get_transaction_status', {
            hash: transaction_hash,
        });
        return transactionStatusResponse;
    }
};

module.exports = Near; 

