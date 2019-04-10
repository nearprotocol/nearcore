const fs = require('fs');
const keyDir = './neardev';
const KeyPair = require('./key_pair');
const {promisify} = require('util');

/**
 * Unencrypted file system key store.
 */
class UnencryptedFileSystemKeyStore {
    constructor(keyDir, networkId) {
        this.keyDir = keyDir;
        this.networkId = networkId;
    }

    /**
     * Set a key for an account with a given id on a given network.
     * @param {string} accountId 
     * @param {string} keypair 
     */
    async setKey(accountId, keypair) {
        if (!await promisify(fs.exists)(keyDir)){
            await promisify(fs.mkdir)(keyDir);
        }
        const keyFileContent = {
            public_key: keypair.getPublicKey(),
            secret_key: keypair.getSecretKey(),
            account_id: accountId,
            netowrk_id: this.networkId
        };
        await promisify(fs.writeFile)(this.getKeyFilePath(accountId), JSON.stringify(keyFileContent));
    }

    /**
     * Get a single key for an account on a given network.
     * @param {string} accountId 
     */
    async getKey(accountId) {
        // Find keys/account id
        if (!await promisify(fs.exists)(this.getKeyFilePath(accountId))) {
            return null;
        }
        const json = await this.getRawKey(accountId);
        return this.getPublicKeyFromJSON(json);
    }

    /**
     * Returns all account ids for a particular network.
     */
    async getAccountIds() {
        if (!await promisify(fs.exists)(keyDir)) {
            return [];
        }
        const result = [];
        const dir = await promisify(fs.readdir)(keyDir);
        for (let i = 0; i < dir.length; i++) {
            if (dir[i].startsWith(this.networkId + '_')) {
                result.push(dir[i].substring(this.networkId.length + 1));
            }
        }
        return result;
    }

    async clear() {
        this.keys = {};
    }

    /**
     * Returns the key file path. The convention is to store the keys in file {networkId}.json
     */
    getKeyFilePath(accountId) {
        return keyDir + '/' + this.networkId + '_' + accountId;
    }

    getPublicKeyFromJSON(json) {
        if (!json.public_key || !json.secret_key || !json.account_id) {
            throw new Error('Deployment failed. neardev/devkey.json format problem. Please make sure file contains public_key, secret_key, and account_id".');
        }
        const result = new KeyPair(json.public_key, json.secret_key);
        return result;
    }

    async getRawKey(accountId) {
        return JSON.parse(await promisify(fs.readFile)(this.getKeyFilePath(accountId)));
    }
}

module.exports = UnencryptedFileSystemKeyStore;