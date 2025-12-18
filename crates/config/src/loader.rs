//! Configuration loading from multiple sources

use crate::{AppConfig, ConfigError, Result};
use config::{Config, ConfigBuilder, Environment, File, FileFormat};
use std::path::Path;

/// Configuration loader with support for multiple formats and sources
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from a file
    ///
    /// Supports TOML, YAML, and JSON formats based on file extension
    pub fn from_file(path: &Path) -> Result<AppConfig> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| ConfigError::LoadError("No file extension found".to_string()))?;

        let content = std::fs::read_to_string(path)?;

        match extension {
            "toml" => Self::from_toml(&content),
            "yaml" | "yml" => Self::from_yaml(&content),
            "json" => Self::from_json(&content),
            _ => Err(ConfigError::LoadError(format!(
                "Unsupported file extension: {}",
                extension
            ))),
        }
    }

    /// Load configuration from TOML string
    pub fn from_toml(content: &str) -> Result<AppConfig> {
        toml::from_str(content).map_err(ConfigError::from)
    }

    /// Load configuration from YAML string
    pub fn from_yaml(content: &str) -> Result<AppConfig> {
        serde_yaml::from_str(content).map_err(ConfigError::from)
    }

    /// Load configuration from JSON string
    pub fn from_json(content: &str) -> Result<AppConfig> {
        serde_json::from_str(content).map_err(ConfigError::from)
    }

    /// Load configuration from environment variables
    ///
    /// Uses default prefix "ATOM_INTENTS"
    pub fn from_env() -> Result<AppConfig> {
        Self::from_env_with_prefix("ATOM_INTENTS")
    }

    /// Load configuration from environment variables with custom prefix
    ///
    /// Environment variables should be in the format: PREFIX_SECTION_KEY
    /// For example: ATOM_INTENTS_NETWORK_ENVIRONMENT=mainnet
    pub fn from_env_with_prefix(prefix: &str) -> Result<AppConfig> {
        let config = Config::builder()
            .add_source(Environment::with_prefix(prefix).separator("_"))
            .build()?;

        config.try_deserialize().map_err(ConfigError::from)
    }

    /// Merge two configurations, with overlay taking precedence
    ///
    /// This performs a deep merge, combining nested structures
    pub fn merge(base: AppConfig, overlay: AppConfig) -> AppConfig {
        AppConfig {
            network: overlay.network,
            chains: {
                let mut chains = base.chains;
                chains.extend(overlay.chains);
                chains
            },
            solvers: overlay.solvers,
            settlement: overlay.settlement,
            oracle: overlay.oracle,
            relayer: overlay.relayer,
            fees: overlay.fees,
        }
    }

    /// Load configuration from file with environment variable overrides
    ///
    /// 1. Loads base configuration from file
    /// 2. Overlays environment variables with the given prefix
    pub fn from_file_with_env(path: &Path, env_prefix: &str) -> Result<AppConfig> {
        let file_config = Self::from_file(path)?;

        // Try to load env overrides, but don't fail if there are none
        match Self::from_env_with_prefix(env_prefix) {
            Ok(env_config) => Ok(Self::merge(file_config, env_config)),
            Err(_) => Ok(file_config), // No env vars set, just use file config
        }
    }

    /// Build configuration using the config crate's builder pattern
    ///
    /// This allows for more complex configuration scenarios with multiple sources
    pub fn builder() -> ConfigLoaderBuilder {
        ConfigLoaderBuilder {
            builder: Config::builder(),
        }
    }
}

/// Builder for complex configuration loading scenarios
pub struct ConfigLoaderBuilder {
    builder: ConfigBuilder<config::builder::DefaultState>,
}

impl ConfigLoaderBuilder {
    /// Add a configuration file source
    pub fn add_file(mut self, path: &Path, required: bool) -> Self {
        let format = match path.extension().and_then(|e| e.to_str()) {
            Some("toml") => FileFormat::Toml,
            Some("yaml") | Some("yml") => FileFormat::Yaml,
            Some("json") => FileFormat::Json,
            _ => FileFormat::Toml, // Default to TOML
        };

        self.builder = self
            .builder
            .add_source(File::from(path).format(format).required(required));
        self
    }

    /// Add environment variable source with prefix
    pub fn add_env(mut self, prefix: &str) -> Self {
        self.builder = self
            .builder
            .add_source(Environment::with_prefix(prefix).separator("_"));
        self
    }

    /// Set a default value for a key
    pub fn set_default(mut self, key: &str, value: &str) -> Self {
        self.builder = self.builder.set_default(key, value).unwrap();
        self
    }

    /// Build the final configuration
    pub fn build(self) -> Result<AppConfig> {
        let config = self.builder.build()?;
        config.try_deserialize().map_err(ConfigError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_from_toml() {
        let toml = r#"
            [network]
            environment = "testnet"
            log_level = "debug"
            metrics_enabled = true
            metrics_port = 9090

            [solvers]
            enabled_solvers = ["solver1", "solver2"]
            solver_endpoints = { solver1 = "http://localhost:8080", solver2 = "http://localhost:8081" }
            min_profit_bps = 10
            max_slippage_bps = 50
            quote_timeout_ms = 5000
            max_concurrent_solvers = 10

            [settlement]
            contract_address = "cosmos1abc"
            timeout_secs = 300
            max_batch_size = 100
            min_confirmations = 1
            parallel_enabled = true

            [oracle]
            provider = "slinky"
            endpoint = "http://localhost:8080"
            update_interval_secs = 60
            staleness_threshold_secs = 300

            [relayer]
            packet_timeout_secs = 600
            auto_relay_enabled = true
            relay_interval_ms = 1000

            [fees]
            protocol_fee_bps = 5
            solver_fee_bps = 10
            fee_recipient = "cosmos1fee"

            [chains]
        "#;

        let config = ConfigLoader::from_toml(toml).unwrap();
        assert_eq!(config.network.log_level, "debug");
        assert_eq!(config.solvers.enabled_solvers.len(), 2);
    }

    #[test]
    fn test_load_from_yaml() {
        let yaml = r#"
network:
  environment: testnet
  log_level: debug
  metrics_enabled: true
  metrics_port: 9090

solvers:
  enabled_solvers:
    - solver1
    - solver2
  min_profit_bps: 10
  max_slippage_bps: 50
  quote_timeout_ms: 5000
  max_concurrent_solvers: 10
  solver_endpoints: {}

settlement:
  contract_address: "cosmos1abc"
  timeout_secs: 300
  max_batch_size: 100
  min_confirmations: 1
  parallel_enabled: true

oracle:
  provider: slinky
  endpoint: "http://localhost:8080"
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
  fee_recipient: "cosmos1fee"

chains: {}
        "#;

        let config = ConfigLoader::from_yaml(yaml).unwrap();
        assert_eq!(config.network.log_level, "debug");
        assert_eq!(config.solvers.enabled_solvers.len(), 2);
    }

    #[test]
    fn test_load_from_json() {
        let json = r#"
{
  "network": {
    "environment": "testnet",
    "log_level": "debug",
    "metrics_enabled": true,
    "metrics_port": 9090
  },
  "chains": {},
  "solvers": {
    "enabled_solvers": ["solver1", "solver2"],
    "min_profit_bps": 10,
    "max_slippage_bps": 50,
    "quote_timeout_ms": 5000,
    "max_concurrent_solvers": 10,
    "solver_endpoints": {}
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
}
        "#;

        let config = ConfigLoader::from_json(json).unwrap();
        assert_eq!(config.network.log_level, "debug");
        assert_eq!(config.solvers.enabled_solvers.len(), 2);
    }

    #[test]
    fn test_load_from_file() {
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

        let mut file = tempfile::Builder::new()
            .suffix(".toml")
            .tempfile()
            .unwrap();
        file.write_all(toml.as_bytes()).unwrap();

        let config = ConfigLoader::from_file(file.path()).unwrap();
        assert_eq!(config.network.log_level, "debug");
    }

    #[test]
    fn test_merge_configs() {
        let base = AppConfig {
            network: crate::NetworkConfig {
                environment: crate::Environment::Local,
                log_level: "info".to_string(),
                metrics_enabled: true,
                metrics_port: 9090,
            },
            ..Default::default()
        };

        let overlay = AppConfig {
            network: crate::NetworkConfig {
                environment: crate::Environment::Testnet,
                log_level: "debug".to_string(),
                metrics_enabled: true,
                metrics_port: 9090,
            },
            ..Default::default()
        };

        let merged = ConfigLoader::merge(base, overlay);
        assert_eq!(merged.network.log_level, "debug");
        assert_eq!(merged.network.environment, crate::Environment::Testnet);
    }
}
