#!/bin/bash
set -ex

./scripts/start_near.sh &
export NEAR_PID=$!
trap 'pkill -15 -P $NEAR_PID' 0

./scripts/waitonserver.sh
#./scripts/build_wasm.sh

# Run nearlib tests
rm -rf nearlib
git clone --single-branch --branch nightshade https://github.com/nearprotocol/nearlib.git nearlib
cd nearlib
export NEAR_PROTOS_DIR="../core/protos/protos"
export HELLO_WASM_PATH="../tests/hello.wasm"
yarn
yarn build
yarn test
yarn doc
cd ..

# Try creating and building new project using NEAR CLI tools
git clone --single-branch --branch nightshade https://git@github.com/nearprotocol/near-shell.git near-shell
cd near-shell
yarn
#yarn test
cd ..