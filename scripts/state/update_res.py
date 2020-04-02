#!/usr/bin/env python3

# update_res.py: update near/res/genesis_config.json to be current `near init` without records
# update_res.py check: check near/res/genesis_config.json matches current `near init`

import subprocess
import sys
import json
import os
from collections import OrderedDict

def main():
    if len(sys.argv) == 1:
        update_res()
    elif len(sys.argv) == 2 and sys.argv[1] == 'check':
        check_res()
    else:
        print('Usage: update-res.py | update-res.py check')
        exit(2)

genesis_config_path = os.path.join(os.path.dirname(os.path.realpath(__file__)), '../../near/res/genesis_config.json')

def near_init_genesis():
    subprocess.check_output('rm -rf /tmp/near/update_res && mkdir -p /tmp/near/update_res', shell=True)
    subprocess.check_output('cargo run -p near -- --home /tmp/near/update_res init --chain-id sample', shell=True)
    genesis = json.load(open('/tmp/near/update_res/genesis.json'), object_pairs_hook=OrderedDict)
    genesis['records'] = []
    # To avoid near/res/genesis_config.json doesn't change everytime
    genesis['genesis_time'] = '1970-01-01T00:00:00.000000000Z'
    return genesis

def update_res():
    genesis = near_init_genesis()
    json.dump(genesis, open(genesis_config_path, 'w'), indent=2)
    print('near/res/genesis_config.json updated')

def check_res():
    genesis = near_init_genesis()
    res_genesis_config = json.load(open(genesis_config_path), object_pairs_hook=OrderedDict)
    if genesis != res_genesis_config:
        print('near/res/genesis_config.json does not match `near init` generated')
        print('Please update by run scripts/state/update_res.py')
        exit(1)


if __name__ == '__main__':
    main()