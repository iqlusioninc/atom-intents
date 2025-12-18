//! Core configuration structures for the ATOM Intent-Based Liquidity System

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct AppConfig {
    /// Network configuration
    pub network: NetworkConfig,

    /// Chain configurations by chain ID
    #[serde(default)]
    pub chains: HashMap<String, ChainConfig>,

    /// Solver configuration
    pub solvers: SolverConfig,

    /// Settlement configuration
    pub settlement: SettlementConfig,

    /// Oracle configuration
    pub oracle: OracleConfig,

    /// Relayer configuration
    pub relayer: RelayerConfig,

    /// Fee configuration
    pub fees: FeeConfig,
}

/// Network environment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Environment type (mainnet, testnet, local)
    pub environment: Environment,

    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Enable metrics collection
    #[serde(default = "default_true")]
    pub metrics_enabled: bool,

    /// Metrics server port
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,
}

/// Environment types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Mainnet,
    Testnet,
    Local,
}

/// Configuration for a blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Chain identifier
    pub chain_id: String,

    /// RPC endpoint URL
    pub rpc_url: String,

    /// gRPC endpoint URL (optional)
    pub grpc_url: Option<String>,

    /// Gas price (e.g., "0.025uatom")
    pub gas_price: String,

    /// Fee denomination
    pub fee_denom: String,

    /// Address prefix (e.g., "cosmos")
    pub address_prefix: String,

    /// Gas adjustment multiplier
    #[serde(default = "default_gas_adjustment")]
    pub gas_adjustment: f64,

    /// Request timeout in milliseconds
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,

    /// Maximum retry attempts
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

/// Solver configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverConfig {
    /// List of enabled solver IDs
    pub enabled_solvers: Vec<String>,

    /// Minimum profit in basis points
    #[serde(default = "default_min_profit_bps")]
    pub min_profit_bps: u64,

    /// Maximum slippage in basis points
    #[serde(default = "default_max_slippage_bps")]
    pub max_slippage_bps: u64,

    /// Quote request timeout in milliseconds
    #[serde(default = "default_quote_timeout_ms")]
    pub quote_timeout_ms: u64,

    /// Maximum concurrent solver requests
    #[serde(default = "default_max_concurrent_solvers")]
    pub max_concurrent_solvers: usize,

    /// Solver endpoints by solver ID
    #[serde(default)]
    pub solver_endpoints: HashMap<String, String>,
}

/// Settlement configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementConfig {
    /// Settlement contract address
    pub contract_address: String,

    /// Settlement timeout in seconds
    #[serde(default = "default_settlement_timeout_secs")]
    pub timeout_secs: u64,

    /// Maximum batch size for settlements
    #[serde(default = "default_max_batch_size")]
    pub max_batch_size: usize,

    /// Minimum confirmations required
    #[serde(default = "default_min_confirmations")]
    pub min_confirmations: u32,

    /// Enable parallel settlement
    #[serde(default = "default_true")]
    pub parallel_enabled: bool,
}

/// Oracle configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    /// Oracle provider (e.g., "chainlink", "band", "slinky")
    #[serde(default = "default_oracle_provider")]
    pub provider: String,

    /// Oracle endpoint URL
    pub endpoint: String,

    /// Price update interval in seconds
    #[serde(default = "default_price_update_interval_secs")]
    pub update_interval_secs: u64,

    /// Price staleness threshold in seconds
    #[serde(default = "default_price_staleness_secs")]
    pub staleness_threshold_secs: u64,

    /// Fallback oracle endpoints
    #[serde(default)]
    pub fallback_endpoints: Vec<String>,
}

/// Relayer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayerConfig {
    /// IBC channel configurations
    #[serde(default)]
    pub channels: HashMap<String, IbcChannelConfig>,

    /// Packet timeout in seconds
    #[serde(default = "default_packet_timeout_secs")]
    pub packet_timeout_secs: u64,

    /// Enable automatic relay
    #[serde(default = "default_true")]
    pub auto_relay_enabled: bool,

    /// Relay interval in milliseconds
    #[serde(default = "default_relay_interval_ms")]
    pub relay_interval_ms: u64,
}

/// IBC channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbcChannelConfig {
    /// Source chain ID
    pub source_chain: String,

    /// Destination chain ID
    pub destination_chain: String,

    /// Channel ID
    pub channel_id: String,

    /// Port ID
    #[serde(default = "default_port_id")]
    pub port_id: String,

    /// Connection ID
    pub connection_id: String,
}

/// Fee configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
    /// Protocol fee in basis points
    #[serde(default = "default_protocol_fee_bps")]
    pub protocol_fee_bps: u64,

    /// Solver fee in basis points
    #[serde(default = "default_solver_fee_bps")]
    pub solver_fee_bps: u64,

    /// Fee recipient address
    pub fee_recipient: String,

    /// Minimum fee amount
    #[serde(default)]
    pub min_fee_amount: Option<String>,
}

// Default value functions
fn default_log_level() -> String {
    "info".to_string()
}

fn default_true() -> bool {
    true
}

fn default_metrics_port() -> u16 {
    9090
}

fn default_gas_adjustment() -> f64 {
    1.3
}

fn default_timeout_ms() -> u64 {
    30000
}

fn default_max_retries() -> u32 {
    3
}

fn default_min_profit_bps() -> u64 {
    10 // 0.1%
}

fn default_max_slippage_bps() -> u64 {
    50 // 0.5%
}

fn default_quote_timeout_ms() -> u64 {
    5000
}

fn default_max_concurrent_solvers() -> usize {
    10
}

fn default_settlement_timeout_secs() -> u64 {
    300 // 5 minutes
}

fn default_max_batch_size() -> usize {
    100
}

fn default_min_confirmations() -> u32 {
    1
}

fn default_price_update_interval_secs() -> u64 {
    60 // 1 minute
}

fn default_price_staleness_secs() -> u64 {
    300 // 5 minutes
}

fn default_packet_timeout_secs() -> u64 {
    600 // 10 minutes
}

fn default_relay_interval_ms() -> u64 {
    1000 // 1 second
}

fn default_port_id() -> String {
    "transfer".to_string()
}

fn default_protocol_fee_bps() -> u64 {
    5 // 0.05%
}

fn default_solver_fee_bps() -> u64 {
    10 // 0.1%
}

fn default_oracle_provider() -> String {
    "slinky".to_string()
}


impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            environment: Environment::Local,
            log_level: default_log_level(),
            metrics_enabled: default_true(),
            metrics_port: default_metrics_port(),
        }
    }
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            enabled_solvers: Vec::new(),
            min_profit_bps: default_min_profit_bps(),
            max_slippage_bps: default_max_slippage_bps(),
            quote_timeout_ms: default_quote_timeout_ms(),
            max_concurrent_solvers: default_max_concurrent_solvers(),
            solver_endpoints: HashMap::new(),
        }
    }
}

impl Default for SettlementConfig {
    fn default() -> Self {
        Self {
            contract_address: String::new(),
            timeout_secs: default_settlement_timeout_secs(),
            max_batch_size: default_max_batch_size(),
            min_confirmations: default_min_confirmations(),
            parallel_enabled: default_true(),
        }
    }
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            provider: "slinky".to_string(),
            endpoint: String::new(),
            update_interval_secs: default_price_update_interval_secs(),
            staleness_threshold_secs: default_price_staleness_secs(),
            fallback_endpoints: Vec::new(),
        }
    }
}

impl Default for RelayerConfig {
    fn default() -> Self {
        Self {
            channels: HashMap::new(),
            packet_timeout_secs: default_packet_timeout_secs(),
            auto_relay_enabled: default_true(),
            relay_interval_ms: default_relay_interval_ms(),
        }
    }
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            protocol_fee_bps: default_protocol_fee_bps(),
            solver_fee_bps: default_solver_fee_bps(),
            fee_recipient: String::new(),
            min_fee_amount: None,
        }
    }
}
