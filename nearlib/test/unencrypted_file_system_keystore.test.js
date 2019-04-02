const UnencryptedFileSystemKeyStore = require('../signing/unencrypted_file_system_keystore.js');
const KeyPair = require('../signing/key_pair.js')

const NETWORK_ID_SINGLE_KEY = "singlekeynetworkid";
const ACCOUNT_ID_SINGLE_KEY = "singlekeyaccountid";
const KEYPAIR_SINGLE_KEY = new KeyPair("public", "secret");

describe('Unencrypted file system keystore', () => {
    let keyStore;

    beforeAll(async () => {
        keyStore = new UnencryptedFileSystemKeyStore("../tests");
        await keyStore.setKey(ACCOUNT_ID_SINGLE_KEY, KEYPAIR_SINGLE_KEY, NETWORK_ID_SINGLE_KEY);
    });

    test('Get all keys with empty network returns empty list', async () => {
        const emptyList = await keyStore.getAccountIds("netowrk");
        expect(emptyList).toEqual([]);
    });  
    
    test('Get all keys with single key in keystore', async () => {
        const accountIds = await keyStore.getAccountIds(NETWORK_ID_SINGLE_KEY);
        expect(accountIds).toEqual([ACCOUNT_ID_SINGLE_KEY]);
    });

    test('Get account id from empty keystore', async () => {
        try {
            const key = await keyStore.getKey("someaccount", "somenetowrk");
            fail("key lookup should have failed trying to lookup an invalid account");
        } catch (e) {
            expect(e).toEqual("Key lookup failed. Please make sure you set up an account.");
        }
    });

    test('Get account id from a network with single key', async () => {
        const key = await keyStore.getKey(ACCOUNT_ID_SINGLE_KEY, NETWORK_ID_SINGLE_KEY);
        expect(key).toEqual(KEYPAIR_SINGLE_KEY);
    });

    test('Add two keys to network and retrieve them', async () => {
        const netoworkId = "twoKeyNetwork";
        const accountId1 = "acc1";
        const accountId2 = "acc2";
        const key1Expected = new KeyPair("p1", "s1");
        const key2Expected = new KeyPair("p2", "s2");
        await keyStore.setKey(accountId1, key1Expected, netoworkId);
        await keyStore.setKey(accountId2, key2Expected, netoworkId);
        const key1 = await keyStore.getKey(accountId1, netoworkId);
        const key2 = await keyStore.getKey(accountId2, netoworkId);
        expect(key1).toEqual(key1Expected);
        expect(key2).toEqual(key2Expected);
        const accountIds = await keyStore.getAccountIds(netoworkId);
        expect(accountIds).toEqual([accountId1, accountId2]);
    });
});
