//! Configuration for execution backends

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Chain configuration for connecting to real networks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: String,
    pub rpc_url: String,
    #[serde(default)]
    pub rest_url: Option<String>,
    #[serde(default)]
    pub grpc_url: Option<String>,
    #[serde(default = "default_gas_price")]
    pub gas_price: String,
    #[serde(default = "default_fee_denom")]
    pub fee_denom: String,
    #[serde(default = "default_account_prefix")]
    pub account_prefix: String,
    #[serde(default = "default_gas_adjustment")]
    pub gas_adjustment: f64,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_gas_price() -> String {
    "0.025".to_string()
}
fn default_fee_denom() -> String {
    "uatom".to_string()
}
fn default_account_prefix() -> String {
    "cosmos".to_string()
}
fn default_gas_adjustment() -> f64 {
    1.5
}
fn default_timeout_ms() -> u64 {
    30000
}
fn default_max_retries() -> u32 {
    3
}

/// Contract deployment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractConfig {
    /// Settlement contract address
    pub settlement_address: String,
    /// Escrow contract address
    pub escrow_address: String,
}

/// IBC channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbcChannelConfig {
    pub source_chain: String,
    pub dest_chain: String,
    pub channel_id: String,
    pub port_id: String,
}

/// Full testnet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestnetConfig {
    /// Primary chain for settlement
    pub primary_chain: String,
    /// Chain configurations by chain_id
    pub chains: HashMap<String, ChainConfig>,
    /// Contract addresses
    pub contracts: ContractConfig,
    /// IBC channels
    #[serde(default)]
    pub ibc_channels: Vec<IbcChannelConfig>,
    /// Settlement timeout in seconds
    #[serde(default = "default_settlement_timeout")]
    pub settlement_timeout_secs: u64,
    /// Enable debug logging
    #[serde(default)]
    pub debug: bool,
}

fn default_settlement_timeout() -> u64 {
    600
}

impl TestnetConfig {
    /// Load configuration from a TOML file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            ConfigError::LoadError(format!("Failed to read config file: {}", e))
        })?;

        toml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(format!("Failed to parse config: {}", e)))
    }

    /// Load from environment variables (for CI/CD)
    pub fn from_env() -> Result<Self, ConfigError> {
        let primary_chain =
            std::env::var("TESTNET_CHAIN_ID").unwrap_or_else(|_| "theta-testnet-001".to_string());

        let rpc_url = std::env::var("TESTNET_RPC_URL").map_err(|_| {
            ConfigError::MissingEnvVar("TESTNET_RPC_URL".to_string())
        })?;

        let settlement_address = std::env::var("SETTLEMENT_CONTRACT").map_err(|_| {
            ConfigError::MissingEnvVar("SETTLEMENT_CONTRACT".to_string())
        })?;

        let escrow_address = std::env::var("ESCROW_CONTRACT").map_err(|_| {
            ConfigError::MissingEnvVar("ESCROW_CONTRACT".to_string())
        })?;

        let mut chains = HashMap::new();
        chains.insert(
            primary_chain.clone(),
            ChainConfig {
                chain_id: primary_chain.clone(),
                rpc_url,
                rest_url: std::env::var("TESTNET_REST_URL").ok(),
                grpc_url: std::env::var("TESTNET_GRPC_URL").ok(),
                gas_price: std::env::var("GAS_PRICE").unwrap_or_else(|_| "0.025".to_string()),
                fee_denom: std::env::var("FEE_DENOM").unwrap_or_else(|_| "uatom".to_string()),
                account_prefix: std::env::var("ACCOUNT_PREFIX")
                    .unwrap_or_else(|_| "cosmos".to_string()),
                gas_adjustment: std::env::var("GAS_ADJUSTMENT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1.5),
                timeout_ms: std::env::var("TIMEOUT_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30000),
                max_retries: std::env::var("MAX_RETRIES")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3),
            },
        );

        Ok(Self {
            primary_chain,
            chains,
            contracts: ContractConfig {
                settlement_address,
                escrow_address,
            },
            ibc_channels: vec![],
            settlement_timeout_secs: std::env::var("SETTLEMENT_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(600),
            debug: std::env::var("DEBUG")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        })
    }

    /// Create a default localnet configuration
    pub fn localnet_default() -> Self {
        let mut chains = HashMap::new();

        chains.insert(
            "localhub-1".to_string(),
            ChainConfig {
                chain_id: "localhub-1".to_string(),
                rpc_url: "http://localhost:26657".to_string(),
                rest_url: Some("http://localhost:1317".to_string()),
                grpc_url: Some("http://localhost:9090".to_string()),
                gas_price: "0.025".to_string(),
                fee_denom: "uatom".to_string(),
                account_prefix: "cosmos".to_string(),
                gas_adjustment: 1.3,
                timeout_ms: 10000,
                max_retries: 3,
            },
        );

        chains.insert(
            "localosmo-1".to_string(),
            ChainConfig {
                chain_id: "localosmo-1".to_string(),
                rpc_url: "http://localhost:26667".to_string(),
                rest_url: Some("http://localhost:1327".to_string()),
                grpc_url: Some("http://localhost:9091".to_string()),
                gas_price: "0.025".to_string(),
                fee_denom: "uosmo".to_string(),
                account_prefix: "osmo".to_string(),
                gas_adjustment: 1.3,
                timeout_ms: 10000,
                max_retries: 3,
            },
        );

        Self {
            primary_chain: "localhub-1".to_string(),
            chains,
            contracts: ContractConfig {
                settlement_address: "cosmos14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s4hmalr"
                    .to_string(),
                escrow_address: "cosmos1nc5tatafv6eyq7llkr2gv50ff9e22mnf70qgjlv737ktmt4eswrqvp52rq"
                    .to_string(),
            },
            ibc_channels: vec![IbcChannelConfig {
                source_chain: "localhub-1".to_string(),
                dest_chain: "localosmo-1".to_string(),
                channel_id: "channel-0".to_string(),
                port_id: "transfer".to_string(),
            }],
            settlement_timeout_secs: 3600, // 1 hour for debugging
            debug: true,
        }
    }

    /// Get chain config by chain_id
    pub fn get_chain(&self, chain_id: &str) -> Option<&ChainConfig> {
        self.chains.get(chain_id)
    }

    /// Get the primary chain config
    pub fn primary_chain_config(&self) -> Option<&ChainConfig> {
        self.chains.get(&self.primary_chain)
    }

    /// Find IBC channel between two chains
    pub fn find_channel(&self, source: &str, dest: &str) -> Option<&IbcChannelConfig> {
        self.ibc_channels
            .iter()
            .find(|c| c.source_chain == source && c.dest_chain == dest)
    }
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to load config: {0}")]
    LoadError(String),

    #[error("failed to parse config: {0}")]
    ParseError(String),

    #[error("missing environment variable: {0}")]
    MissingEnvVar(String),

    #[error("invalid configuration: {0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localnet_default() {
        let config = TestnetConfig::localnet_default();
        assert_eq!(config.primary_chain, "localhub-1");
        assert!(config.chains.contains_key("localhub-1"));
        assert!(config.chains.contains_key("localosmo-1"));
    }

    #[test]
    fn test_chain_lookup() {
        let config = TestnetConfig::localnet_default();
        let hub = config.get_chain("localhub-1");
        assert!(hub.is_some());
        assert_eq!(hub.unwrap().rpc_url, "http://localhost:26657");
    }
}
