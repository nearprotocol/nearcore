#!/bin/bash
set -ex

# Must start binary outside of this script.
./scripts/waitonserver.sh
./scripts/build_wasm.sh

# Run nearlib tests
rm -rf nearlib
git clone -b accounting https://github.com/nearprotocol/nearlib.git nearlib
cd nearlib
export NEARCORE_DIR="../"
npm install
npm test
npm run build
npm run doc
cd ..

# Try creating and building new project using NEAR CLI tools
git clone https://git@github.com/nearprotocol/near-shell.git near-shell
cd near-shell
npm install
npm test
cd ..

./scripts/kill_devnet.sh
