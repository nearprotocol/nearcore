use super::{DEFAULT_HOME, NEARD_VERSION, NEARD_VERSION_STRING, PROTOCOL_VERSION};
use clap::{AppSettings, Clap};
use near_primitives::types::{NumSeats, NumShards};
use nearcore::get_store_path;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::{env, fs, io};
#[cfg(feature = "adversarial")]
use tracing::error;
use tracing::info;
use tracing::metadata::LevelFilter;
use tracing_subscriber::EnvFilter;

/// NEAR Protocol Node
#[derive(Clap)]
#[clap(version = NEARD_VERSION_STRING.as_str())]
#[clap(setting = AppSettings::SubcommandRequiredElseHelp)]
pub(super) struct NeardCmd {
    #[clap(flatten)]
    opts: NeardOpts,
    #[clap(subcommand)]
    subcmd: NeardSubCommand,
}

impl NeardCmd {
    pub(super) fn parse_and_run() {
        let neard_cmd = Self::parse();
        neard_cmd.opts.init();
        info!(target: "near", "Version: {}, Build: {}, Latest Protocol: {}", NEARD_VERSION.version, NEARD_VERSION.build, PROTOCOL_VERSION);

        #[cfg(feature = "adversarial")]
        {
            error!("THIS IS A NODE COMPILED WITH ADVERSARIAL BEHAVIORS. DO NOT USE IN PRODUCTION.");

            if env::var("ADVERSARY_CONSENT").unwrap_or_else(|_| "".to_string()) != "1" {
                error!("To run a node with adversarial behavior enabled give your consent by setting variable:");
                error!("ADVERSARY_CONSENT=1");
                std::process::exit(1);
            }
        }

        let home_dir = neard_cmd.opts.home;

        match neard_cmd.subcmd {
            NeardSubCommand::Init(cmd) => cmd.run(&home_dir),
            NeardSubCommand::Testnet(cmd) => cmd.run(&home_dir),
            NeardSubCommand::Run(cmd) => cmd.run(&home_dir),

            NeardSubCommand::UnsafeResetData => {
                let store_path = get_store_path(&home_dir);
                info!(target: "near", "Removing all data from {}", store_path);
                fs::remove_dir_all(store_path).expect("Removing data failed");
            }
            NeardSubCommand::UnsafeResetAll => {
                info!(target: "near", "Removing all data and config from {}", home_dir.to_string_lossy());
                fs::remove_dir_all(home_dir).expect("Removing data and config failed.");
            }
        }
    }
}

#[derive(Clap, Debug)]
struct NeardOpts {
    /// Verbose logging.
    #[clap(long)]
    verbose: Option<Option<String>>,
    /// Directory for config and data (default "~/.near").
    #[clap(long, parse(from_os_str), default_value = &DEFAULT_HOME)]
    home: PathBuf,
}

impl NeardOpts {
    fn init(&self) {
        init_logging(self.verbose.as_ref().map(Option::as_deref).flatten());
    }
}

#[derive(Clap)]
pub(super) enum NeardSubCommand {
    /// Initializes NEAR configuration
    #[clap(name = "init")]
    Init(InitCmd),
    /// Runs NEAR node
    #[clap(name = "run")]
    Run(RunCmd),
    /// Sets up testnet configuration with all necessary files (validator key, node key, genesis
    /// and config)
    #[clap(name = "testnet")]
    Testnet(TestnetCmd),
    /// (unsafe) Remove all the config, keys, data and effectively removing all information about
    /// the network
    #[clap(name = "unsafe_reset_all")]
    UnsafeResetAll,
    /// (unsafe) Remove all the data, effectively resetting node to the genesis state (keeps genesis and
    /// config)
    #[clap(name = "unsafe_reset_data")]
    UnsafeResetData,
}

#[derive(Clap)]
pub(super) struct InitCmd {
    /// Download the verified NEAR genesis file automatically.
    #[clap(long)]
    download_genesis: bool,
    /// Makes block production fast (TESTING ONLY).
    #[clap(long)]
    fast: bool,
    /// Account ID for the validator key.
    #[clap(long)]
    account_id: Option<String>,
    /// Chain ID, by default creates new random.
    #[clap(long)]
    chain_id: Option<String>,
    /// Specify a custom download URL for the genesis-file.
    #[clap(long)]
    download_genesis_url: Option<String>,
    /// Genesis file to use when initializing testnet (including downloading).
    #[clap(long)]
    genesis: Option<String>,
    /// Number of shards to initialize the chain with.
    #[clap(long, default_value = "1")]
    num_shards: NumShards,
    /// Specify private key generated from seed (TESTING ONLY).
    #[clap(long)]
    test_seed: Option<String>,
}

impl InitCmd {
    pub(super) fn run(self, home_dir: &Path) {
        // TODO: Check if `home` exists. If exists check what networks we already have there.
        if (self.download_genesis || self.download_genesis_url.is_some()) && self.genesis.is_some()
        {
            panic!(
                    "Please specify a local genesis file or download the NEAR genesis or specify your own."
                );
        }

        nearcore::init_configs(
            home_dir,
            self.chain_id.as_deref(),
            self.account_id.as_deref(),
            self.test_seed.as_deref(),
            self.num_shards,
            self.fast,
            self.genesis.as_deref(),
            self.download_genesis,
            self.download_genesis_url.as_deref(),
        );
    }
}

#[derive(Clap)]
pub(super) struct RunCmd {
    /// Keep old blocks in the storage (default false).
    #[clap(long)]
    archive: bool,
    /// Set the boot nodes to bootstrap network from.
    #[clap(long)]
    boot_nodes: Option<String>,
    /// Minimum number of peers to start syncing/producing blocks
    #[clap(long)]
    min_peers: Option<usize>,
    /// Customize network listening address (useful for running multiple nodes on the same machine).
    #[clap(long)]
    network_addr: Option<SocketAddr>,
    /// Set this to false to only produce blocks when there are txs or receipts (default true).
    #[clap(long)]
    produce_empty_blocks: Option<bool>,
    /// Customize RPC listening address (useful for running multiple nodes on the same machine).
    #[clap(long)]
    rpc_addr: Option<String>,
    /// Customize telemetry url.
    #[clap(long)]
    telemetry_url: Option<String>,
}

impl RunCmd {
    pub(super) fn run(self, home_dir: &Path) {
        // Load configs from home.
        let mut near_config = nearcore::config::load_config_without_genesis_records(home_dir);
        nearcore::genesis_validate::validate_genesis(&near_config.genesis);
        // Set current version in client config.
        near_config.client_config.version = super::NEARD_VERSION.clone();
        // Override some parameters from command line.
        if let Some(produce_empty_blocks) = self.produce_empty_blocks {
            near_config.client_config.produce_empty_blocks = produce_empty_blocks;
        }
        if let Some(boot_nodes) = self.boot_nodes {
            if !boot_nodes.is_empty() {
                near_config.network_config.boot_nodes = boot_nodes
                    .split(',')
                    .map(|chunk| chunk.parse().expect("Failed to parse PeerInfo"))
                    .collect();
            }
        }
        if let Some(min_peers) = self.min_peers {
            near_config.client_config.min_num_peers = min_peers;
        }
        if let Some(network_addr) = self.network_addr {
            near_config.network_config.addr = Some(network_addr);
        }
        if let Some(rpc_addr) = self.rpc_addr {
            near_config.rpc_config.addr = rpc_addr;
        }
        if let Some(telemetry_url) = self.telemetry_url {
            if !telemetry_url.is_empty() {
                near_config.telemetry_config.endpoints.push(telemetry_url);
            }
        }
        if self.archive {
            near_config.client_config.archive = true;
        }

        let sys = actix::System::new();
        sys.block_on(async move {
            nearcore::start_with_config(home_dir, near_config);
        });
        sys.run().unwrap();
    }
}

#[derive(Clap)]
pub(super) struct TestnetCmd {
    /// Number of non-validators to initialize the testnet with.
    #[clap(long = "n", default_value = "0")]
    non_validators: NumSeats,
    /// Prefix the directory name for each node with (node results in node0, node1, ...)
    #[clap(long, default_value = "node")]
    prefix: String,
    /// Number of shards to initialize the testnet with.
    #[clap(long, default_value = "4")]
    shards: NumShards,
    /// Number of validators to initialize the testnet with.
    #[clap(long = "v", default_value = "4")]
    validators: NumSeats,
}

impl TestnetCmd {
    pub(super) fn run(self, home_dir: &Path) {
        nearcore::config::init_testnet_configs(
            home_dir,
            self.shards,
            self.validators,
            self.non_validators,
            &self.prefix,
            false,
        );
    }
}

fn init_logging(verbose: Option<&str>) {
    let mut env_filter = EnvFilter::new(
        "tokio_reactor=info,near=info,stats=info,telemetry=info,delay_detector=info,\
         near-performance-metrics=info,near-rust-allocator-proxy=info",
    );

    if let Some(module) = verbose {
        env_filter = env_filter
            .add_directive("cranelift_codegen=warn".parse().unwrap())
            .add_directive("cranelift_codegen=warn".parse().unwrap())
            .add_directive("h2=warn".parse().unwrap())
            .add_directive("trust_dns_resolver=warn".parse().unwrap())
            .add_directive("trust_dns_proto=warn".parse().unwrap());

        if module.is_empty() {
            env_filter = env_filter.add_directive(LevelFilter::DEBUG.into());
        } else {
            env_filter = env_filter.add_directive(format!("{}=debug", module).parse().unwrap());
        }
    } else {
        env_filter = env_filter.add_directive(LevelFilter::WARN.into());
    }

    if let Ok(rust_log) = env::var("RUST_LOG") {
        if !rust_log.is_empty() {
            for directive in rust_log.split(',').filter_map(|s| match s.parse() {
                Ok(directive) => Some(directive),
                Err(err) => {
                    eprintln!("Ignoring directive `{}`: {}", s, err);
                    None
                }
            }) {
                env_filter = env_filter.add_directive(directive);
            }
        }
    }
    tracing_subscriber::fmt::Subscriber::builder()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .with_env_filter(env_filter)
        .with_writer(io::stderr)
        .init();
}
