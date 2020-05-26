docker-nearcore:
	DOCKER_BUILDKIT=1 docker build -t nearcore -f Dockerfile .

release:
	cargo build -p neard --release
	cargo build -p keypair-generator
	cargo build -p genesis-csv-to-json
	cargo build -p near-vm-runner-standalone --release
	cargo build -p state-viewer
	cargo build -p store-validator-bin

debug:
	cargo build -p neard
	cargo build -p keypair-generator
	cargo build -p genesis-csv-to-json
	cargo build -p near-vm-runner-standalone
	cargo build -p state-viewer
	cargo build -p store-validator-bin
