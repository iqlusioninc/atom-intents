//! Integration tests for the config crate

use atom_intents_config::{
    AppConfig, ChainConfig, ConfigLoader, Environment, validate_config,
};
use std::collections::HashMap;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_load_mainnet_config() {
    let config = ConfigLoader::from_file(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/mainnet.toml")
            .as_path(),
    )
    .expect("Failed to load mainnet config");

    assert_eq!(config.network.environment, Environment::Mainnet);
    assert!(!config.chains.is_empty());
    assert!(!config.solvers.enabled_solvers.is_empty());
}

#[test]
fn test_load_testnet_config() {
    let config = ConfigLoader::from_file(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/testnet.toml")
            .as_path(),
    )
    .expect("Failed to load testnet config");

    assert_eq!(config.network.environment, Environment::Testnet);
    assert_eq!(config.network.log_level, "debug");
}

#[test]
fn test_load_local_config() {
    let config = ConfigLoader::from_file(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/local.toml")
            .as_path(),
    )
    .expect("Failed to load local config");

    assert_eq!(config.network.environment, Environment::Local);
    assert_eq!(config.network.log_level, "trace");
}

#[test]
fn test_config_validation_valid() {
    let mut chains = HashMap::new();
    chains.insert(
        "cosmoshub".to_string(),
        ChainConfig {
            chain_id: "cosmoshub-4".to_string(),
            rpc_url: "https://rpc.cosmos.network".to_string(),
            grpc_url: Some("https://grpc.cosmos.network".to_string()),
            gas_price: "0.025uatom".to_string(),
            fee_denom: "uatom".to_string(),
            address_prefix: "cosmos".to_string(),
            gas_adjustment: 1.3,
            timeout_ms: 30000,
            max_retries: 3,
        },
    );

    let mut solver_endpoints = HashMap::new();
    solver_endpoints.insert("solver1".to_string(), "http://localhost:8080".to_string());

    let config = AppConfig {
        network: atom_intents_config::NetworkConfig {
            environment: Environment::Testnet,
            log_level: "info".to_string(),
            metrics_enabled: true,
            metrics_port: 9090,
        },
        chains,
        solvers: atom_intents_config::SolverConfig {
            enabled_solvers: vec!["solver1".to_string()],
            solver_endpoints,
            min_profit_bps: 10,
            max_slippage_bps: 50,
            quote_timeout_ms: 5000,
            max_concurrent_solvers: 10,
        },
        settlement: atom_intents_config::SettlementConfig {
            contract_address: "cosmos1abc".to_string(),
            timeout_secs: 300,
            max_batch_size: 100,
            min_confirmations: 1,
            parallel_enabled: true,
        },
        oracle: atom_intents_config::OracleConfig {
            provider: "slinky".to_string(),
            endpoint: "http://localhost:8080".to_string(),
            update_interval_secs: 60,
            staleness_threshold_secs: 300,
            fallback_endpoints: vec![],
        },
        relayer: atom_intents_config::RelayerConfig {
            channels: HashMap::new(),
            packet_timeout_secs: 600,
            auto_relay_enabled: true,
            relay_interval_ms: 1000,
        },
        fees: atom_intents_config::FeeConfig {
            protocol_fee_bps: 5,
            solver_fee_bps: 10,
            fee_recipient: "cosmos1fee".to_string(),
            min_fee_amount: None,
        },
    };

    assert!(validate_config(&config).is_ok());
}

#[test]
fn test_config_validation_invalid_log_level() {
    let config = AppConfig {
        network: atom_intents_config::NetworkConfig {
            environment: Environment::Testnet,
            log_level: "invalid".to_string(),
            metrics_enabled: true,
            metrics_port: 9090,
        },
        solvers: atom_intents_config::SolverConfig {
            enabled_solvers: vec!["solver1".to_string()],
            ..Default::default()
        },
        settlement: atom_intents_config::SettlementConfig {
            contract_address: "cosmos1abc".to_string(),
            ..Default::default()
        },
        oracle: atom_intents_config::OracleConfig {
            endpoint: "http://localhost:8080".to_string(),
            ..Default::default()
        },
        fees: atom_intents_config::FeeConfig {
            fee_recipient: "cosmos1fee".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    assert!(validate_config(&config).is_err());
}

#[test]
fn test_config_merge() {
    let base = AppConfig {
        network: atom_intents_config::NetworkConfig {
            environment: Environment::Local,
            log_level: "info".to_string(),
            metrics_enabled: true,
            metrics_port: 9090,
        },
        ..Default::default()
    };

    let overlay = AppConfig {
        network: atom_intents_config::NetworkConfig {
            environment: Environment::Testnet,
            log_level: "debug".to_string(),
            metrics_enabled: true,
            metrics_port: 9091,
        },
        ..Default::default()
    };

    let merged = ConfigLoader::merge(base, overlay);

    assert_eq!(merged.network.environment, Environment::Testnet);
    assert_eq!(merged.network.log_level, "debug");
    assert_eq!(merged.network.metrics_port, 9091);
}

#[test]
fn test_config_builder() {
    let toml = r#"
[network]
environment = "testnet"
log_level = "debug"

[solvers]
enabled_solvers = ["solver1"]
solver_endpoints = { solver1 = "http://localhost:8080" }

[settlement]
contract_address = "cosmos1abc"

[oracle]
provider = "slinky"
endpoint = "http://localhost:8080"

[relayer]
channels = {}

[fees]
fee_recipient = "cosmos1fee"

[chains]
    "#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(toml.as_bytes()).unwrap();
    file.flush().unwrap();

    let config = ConfigLoader::builder()
        .add_file(file.path(), true)
        .build()
        .expect("Failed to build config");

    assert_eq!(config.network.log_level, "debug");
}

#[test]
fn test_yaml_format() {
    let yaml = r#"
network:
  environment: testnet
  log_level: debug
  metrics_enabled: true
  metrics_port: 9090

chains: {}

solvers:
  enabled_solvers:
    - solver1
  solver_endpoints:
    solver1: http://localhost:8080
  min_profit_bps: 10
  max_slippage_bps: 50
  quote_timeout_ms: 5000
  max_concurrent_solvers: 10

settlement:
  contract_address: cosmos1abc
  timeout_secs: 300
  max_batch_size: 100
  min_confirmations: 1
  parallel_enabled: true

oracle:
  provider: slinky
  endpoint: http://localhost:8080
  update_interval_secs: 60
  staleness_threshold_secs: 300
  fallback_endpoints: []

relayer:
  channels: {}
  packet_timeout_secs: 600
  auto_relay_enabled: true
  relay_interval_ms: 1000

fees:
  protocol_fee_bps: 5
  solver_fee_bps: 10
  fee_recipient: cosmos1fee
    "#;

    let config = ConfigLoader::from_yaml(yaml).expect("Failed to parse YAML");
    assert_eq!(config.network.log_level, "debug");
    assert_eq!(config.solvers.enabled_solvers.len(), 1);
}

#[test]
fn test_json_format() {
    let json = r#"{
  "network": {
    "environment": "testnet",
    "log_level": "debug",
    "metrics_enabled": true,
    "metrics_port": 9090
  },
  "chains": {},
  "solvers": {
    "enabled_solvers": ["solver1"],
    "solver_endpoints": {
      "solver1": "http://localhost:8080"
    },
    "min_profit_bps": 10,
    "max_slippage_bps": 50,
    "quote_timeout_ms": 5000,
    "max_concurrent_solvers": 10
  },
  "settlement": {
    "contract_address": "cosmos1abc",
    "timeout_secs": 300,
    "max_batch_size": 100,
    "min_confirmations": 1,
    "parallel_enabled": true
  },
  "oracle": {
    "provider": "slinky",
    "endpoint": "http://localhost:8080",
    "update_interval_secs": 60,
    "staleness_threshold_secs": 300,
    "fallback_endpoints": []
  },
  "relayer": {
    "channels": {},
    "packet_timeout_secs": 600,
    "auto_relay_enabled": true,
    "relay_interval_ms": 1000
  },
  "fees": {
    "protocol_fee_bps": 5,
    "solver_fee_bps": 10,
    "fee_recipient": "cosmos1fee"
  }
}"#;

    let config = ConfigLoader::from_json(json).expect("Failed to parse JSON");
    assert_eq!(config.network.log_level, "debug");
    assert_eq!(config.solvers.enabled_solvers.len(), 1);
}

#[test]
fn test_default_values() {
    let minimal_toml = r#"
[network]
environment = "local"

[solvers]
enabled_solvers = ["solver1"]
solver_endpoints = { solver1 = "http://localhost:8080" }

[settlement]
contract_address = "cosmos1abc"

[oracle]
endpoint = "http://localhost:8080"

[relayer]
channels = {}

[fees]
fee_recipient = "cosmos1fee"

[chains]
    "#;

    let config = ConfigLoader::from_toml(minimal_toml).expect("Failed to parse TOML");

    // Check default values are applied
    assert_eq!(config.network.log_level, "info");
    assert_eq!(config.network.metrics_enabled, true);
    assert_eq!(config.network.metrics_port, 9090);
    assert_eq!(config.solvers.min_profit_bps, 10);
    assert_eq!(config.solvers.max_slippage_bps, 50);
    assert_eq!(config.settlement.timeout_secs, 300);
}
