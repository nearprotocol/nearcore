# Spins up one validating node. Wait until they reach height 40.
# Start the second validating node and check that the second node can sync up before
# the end of epoch and produce blocks and chunks.

import sys, time

sys.path.append('lib')


from cluster import start_cluster

BLOCK_WAIT = 40

consensus_config = {"consensus": {"block_fetch_horizon": 10, "block_header_fetch_horizon": 10}}
nodes = start_cluster(2, 0, 1, {'local': True, 'near_root': '../target/debug/'}, [["epoch_length", 80], ["block_producer_kickout_threshold", 10], ["chunk_producer_kickout_threshold", 10]], {0: consensus_config, 1: consensus_config})
time.sleep(2)
nodes[0].kill()

cur_height = 0
print("step 1")
while cur_height < BLOCK_WAIT:
    status = nodes[1].get_status()
    cur_height = status['sync_info']['latest_block_height']
    time.sleep(2)
nodes[0].start(nodes[0].node_key.pk, nodes[0].addr())

print("step 2")
synced = False
while cur_height <= 80:
    status0 = nodes[0].get_status()
    block_height0 = status0['sync_info']['latest_block_height']
    block_hash0 = status0['sync_info']['latest_block_hash']
    status1 = nodes[1].get_status()
    block_height1 = status0['sync_info']['latest_block_height']
    block_hash1 = status0['sync_info']['latest_block_hash']
    if block_height0 > BLOCK_WAIT:
        if block_height0 > block_height1:
            try:
                nodes[0].get_block(block_hash1)
                synced = abs(block_height0 - block_height1) < 5
            except Exception:
                pass
        else:
            try:
                nodes[1].get_block(block_hash0)
                synced = abs(block_height0 - block_height1) < 5
            except Exception:
                pass
    cur_height = max(block_height0, block_height1)
    time.sleep(1)

if not synced:
    assert False, "Nodes are not synced"

status = nodes[1].get_status()
validator_info = nodes[1].json_rpc('validators', [status['sync_info']['latest_block_hash']])
if len(validator_info['result']['next_validators']) < 2:
    assert False, "Node 0 did not produce enough blocks"

for i in range(2):
    account0 = nodes[0].get_account("test%s" % i)['result']
    account1 = nodes[1].get_account("test%s" % i)['result']
    assert account0 == account1, "state diverged"
