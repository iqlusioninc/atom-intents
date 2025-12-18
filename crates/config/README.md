# ATOM Intents Config

Centralized configuration management for the ATOM Intent-Based Liquidity System.

## Features

- **Multiple Config Formats**: Support for TOML, YAML, and JSON
- **Environment Variables**: Override config values with environment variables
- **Hot-Reload**: Automatic configuration reloading when files change
- **Validation**: Comprehensive configuration validation
- **Type-Safe**: Strongly-typed configuration structs
- **Default Configs**: Pre-configured settings for mainnet, testnet, and local development

## Usage

### Loading Configuration

```rust
use atom_intents_config::{ConfigLoader, AppConfig};
use std::path::Path;

// Load from file
let config = ConfigLoader::from_file(Path::new("config/mainnet.toml"))?;

// Load from environment variables
let config = ConfigLoader::from_env()?;

// Load from file with environment variable overrides
let config = ConfigLoader::from_file_with_env(
    Path::new("config/mainnet.toml"),
    "ATOM_INTENTS"
)?;

// Use the builder pattern for complex scenarios
let config = ConfigLoader::builder()
    .add_file(Path::new("config/base.toml"), true)
    .add_file(Path::new("config/overrides.toml"), false)
    .add_env("ATOM_INTENTS")
    .build()?;
```

### Validating Configuration

```rust
use atom_intents_config::validate_config;

let config = ConfigLoader::from_file(Path::new("config/mainnet.toml"))?;
validate_config(&config)?;
```

### Hot-Reload Support

```rust
use atom_intents_config::ConfigWatcher;
use std::path::PathBuf;

// Create watcher and start watching
let (watcher, handle) = ConfigWatcher::watch(PathBuf::from("config/local.toml"))?;

// Get current config
let config = watcher.get_config();

// Config will automatically reload when the file changes
```

## Configuration Structure

### Network Configuration

```toml
[network]
environment = "mainnet"  # mainnet, testnet, or local
log_level = "info"       # trace, debug, info, warn, error
metrics_enabled = true
metrics_port = 9090
```

### Chain Configuration

```toml
[chains.cosmoshub]
chain_id = "cosmoshub-4"
rpc_url = "https://rpc.cosmos.network"
grpc_url = "https://grpc.cosmos.network"
gas_price = "0.025uatom"
fee_denom = "uatom"
address_prefix = "cosmos"
gas_adjustment = 1.3
timeout_ms = 30000
max_retries = 3
```

### Solver Configuration

```toml
[solvers]
enabled_solvers = ["skip", "timewave"]
min_profit_bps = 10      # 0.1%
max_slippage_bps = 50    # 0.5%
quote_timeout_ms = 5000
max_concurrent_solvers = 10

[solvers.solver_endpoints]
skip = "https://api.skip.money"
timewave = "https://api.timewave.computer"
```

### Settlement Configuration

```toml
[settlement]
contract_address = "cosmos1..."
timeout_secs = 300
max_batch_size = 100
min_confirmations = 1
parallel_enabled = true
```

### Oracle Configuration

```toml
[oracle]
provider = "slinky"
endpoint = "https://oracle.cosmos.network"
update_interval_secs = 60
staleness_threshold_secs = 300
fallback_endpoints = []
```

### Relayer Configuration

```toml
[relayer]
packet_timeout_secs = 600
auto_relay_enabled = true
relay_interval_ms = 1000

[relayer.channels.cosmoshub_osmosis]
source_chain = "cosmoshub"
destination_chain = "osmosis"
channel_id = "channel-141"
port_id = "transfer"
connection_id = "connection-257"
```

### Fee Configuration

```toml
[fees]
protocol_fee_bps = 5     # 0.05%
solver_fee_bps = 10      # 0.1%
fee_recipient = "cosmos1..."
```

## Environment Variable Overrides

Environment variables follow the pattern: `PREFIX_SECTION_KEY`

Examples:
```bash
ATOM_INTENTS_NETWORK_LOG_LEVEL=debug
ATOM_INTENTS_SOLVERS_MIN_PROFIT_BPS=20
ATOM_INTENTS_ORACLE_ENDPOINT=https://custom-oracle.example.com
```

## Pre-configured Environments

Three default configurations are provided in the `config/` directory:

- **mainnet.toml**: Production configuration for mainnet
- **testnet.toml**: Configuration for testnet environments
- **local.toml**: Development configuration for local testing

## Validation

The config system provides comprehensive validation:

- URL format validation
- Gas price format validation
- Cross-references (e.g., IBC channels reference existing chains)
- Value range checks (e.g., basis points <= 10000)
- Required field checks

## Testing

Run tests with:

```bash
cargo test -p atom-intents-config
```

## License

Apache-2.0
