#!/bin/bash
set -ex

# Must start binary outside of this script.
./scripts/waitonserver.sh
./scripts/build_wasm.sh

# Run nearlib tests
rm -rf nearlib
git clone https://github.com/nearprotocol/nearlib.git nearlib
cd nearlib
export NEARCORE_DIR="../"
npm install
npm test
npm run build
npm run doc
cd ..

# Try creating and building new project using NEAR CLI tools
rm -rf new_project
mkdir new_project
cd new_project
npm install git+https://git@github.com/nearprotocol/near-shell.git
$(npm bin)/near new_project
# Disabled running create_account / test, as it's currently deploys to general devnet instead of local.
# $(npm bin)/near create_account --account_id=near-hello-devnet
npm install
npm run build
# npm test
cd ..

./scripts/kill_devnet.sh
