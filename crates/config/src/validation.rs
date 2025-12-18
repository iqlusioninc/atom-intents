//! Configuration validation

use crate::{AppConfig, ChainConfig, ConfigError, Result};
use std::collections::HashSet;

/// Validation error details
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// Validate the entire application configuration
pub fn validate_config(config: &AppConfig) -> Result<()> {
    let mut errors = Vec::new();

    // Validate network config
    if let Err(e) = validate_log_level(&config.network.log_level) {
        errors.push(e);
    }

    if config.network.metrics_port == 0 {
        errors.push(ValidationError::new(
            "network.metrics_port",
            "metrics port must be greater than 0",
        ));
    }

    // Validate chains
    for (chain_name, chain_config) in &config.chains {
        if let Err(e) = validate_chain_config(chain_config) {
            errors.push(ValidationError::new(
                format!("chains.{chain_name}"),
                e.to_string(),
            ));
        }
    }

    // Validate solvers
    if config.solvers.enabled_solvers.is_empty() {
        errors.push(ValidationError::new(
            "solvers.enabled_solvers",
            "at least one solver must be enabled",
        ));
    }

    // Check for duplicate solver IDs
    let solver_ids: HashSet<_> = config.solvers.enabled_solvers.iter().collect();
    if solver_ids.len() != config.solvers.enabled_solvers.len() {
        errors.push(ValidationError::new(
            "solvers.enabled_solvers",
            "duplicate solver IDs found",
        ));
    }

    // Validate solver endpoints for all enabled solvers
    for solver_id in &config.solvers.enabled_solvers {
        if !config.solvers.solver_endpoints.contains_key(solver_id) {
            errors.push(ValidationError::new(
                format!("solvers.solver_endpoints.{solver_id}"),
                "enabled solver missing endpoint configuration",
            ));
        }
    }

    if config.solvers.min_profit_bps > 10000 {
        errors.push(ValidationError::new(
            "solvers.min_profit_bps",
            "must be <= 10000 (100%)",
        ));
    }

    if config.solvers.max_slippage_bps > 10000 {
        errors.push(ValidationError::new(
            "solvers.max_slippage_bps",
            "must be <= 10000 (100%)",
        ));
    }

    if config.solvers.quote_timeout_ms == 0 {
        errors.push(ValidationError::new(
            "solvers.quote_timeout_ms",
            "must be greater than 0",
        ));
    }

    if config.solvers.max_concurrent_solvers == 0 {
        errors.push(ValidationError::new(
            "solvers.max_concurrent_solvers",
            "must be greater than 0",
        ));
    }

    // Validate settlement config
    if config.settlement.contract_address.is_empty() {
        errors.push(ValidationError::new(
            "settlement.contract_address",
            "contract address is required",
        ));
    }

    if config.settlement.timeout_secs == 0 {
        errors.push(ValidationError::new(
            "settlement.timeout_secs",
            "must be greater than 0",
        ));
    }

    if config.settlement.max_batch_size == 0 {
        errors.push(ValidationError::new(
            "settlement.max_batch_size",
            "must be greater than 0",
        ));
    }

    // Validate oracle config
    if config.oracle.endpoint.is_empty() {
        errors.push(ValidationError::new(
            "oracle.endpoint",
            "oracle endpoint is required",
        ));
    }

    if let Err(e) = validate_url(&config.oracle.endpoint) {
        errors.push(ValidationError::new("oracle.endpoint", e));
    }

    for (idx, fallback) in config.oracle.fallback_endpoints.iter().enumerate() {
        if let Err(e) = validate_url(fallback) {
            errors.push(ValidationError::new(
                format!("oracle.fallback_endpoints[{idx}]"),
                e,
            ));
        }
    }

    if config.oracle.update_interval_secs == 0 {
        errors.push(ValidationError::new(
            "oracle.update_interval_secs",
            "must be greater than 0",
        ));
    }

    // Validate relayer config
    for (channel_name, channel_config) in &config.relayer.channels {
        if channel_config.channel_id.is_empty() {
            errors.push(ValidationError::new(
                format!("relayer.channels.{channel_name}.channel_id"),
                "channel ID is required",
            ));
        }

        if channel_config.connection_id.is_empty() {
            errors.push(ValidationError::new(
                format!("relayer.channels.{channel_name}.connection_id"),
                "connection ID is required",
            ));
        }

        // Verify referenced chains exist
        if !config.chains.contains_key(&channel_config.source_chain) {
            errors.push(ValidationError::new(
                format!("relayer.channels.{channel_name}.source_chain"),
                format!(
                    "chain '{}' not found in chains config",
                    channel_config.source_chain
                ),
            ));
        }

        if !config
            .chains
            .contains_key(&channel_config.destination_chain)
        {
            errors.push(ValidationError::new(
                format!("relayer.channels.{channel_name}.destination_chain"),
                format!(
                    "chain '{}' not found in chains config",
                    channel_config.destination_chain
                ),
            ));
        }
    }

    // Validate fee config
    if config.fees.protocol_fee_bps > 10000 {
        errors.push(ValidationError::new(
            "fees.protocol_fee_bps",
            "must be <= 10000 (100%)",
        ));
    }

    if config.fees.solver_fee_bps > 10000 {
        errors.push(ValidationError::new(
            "fees.solver_fee_bps",
            "must be <= 10000 (100%)",
        ));
    }

    if config.fees.fee_recipient.is_empty() {
        errors.push(ValidationError::new(
            "fees.fee_recipient",
            "fee recipient address is required",
        ));
    }

    // Return all errors if any were found
    if !errors.is_empty() {
        let error_msg = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ConfigError::ValidationError(error_msg));
    }

    Ok(())
}

/// Validate a chain configuration
pub fn validate_chain_config(chain: &ChainConfig) -> std::result::Result<(), String> {
    if chain.chain_id.is_empty() {
        return Err("chain_id is required".to_string());
    }

    if chain.rpc_url.is_empty() {
        return Err("rpc_url is required".to_string());
    }

    validate_url(&chain.rpc_url)?;

    if let Some(grpc_url) = &chain.grpc_url {
        validate_url(grpc_url)?;
    }

    if chain.gas_price.is_empty() {
        return Err("gas_price is required".to_string());
    }

    // Validate gas price format (should be like "0.025uatom")
    if !chain.gas_price.chars().any(|c| c.is_ascii_alphabetic()) {
        return Err("gas_price must include denomination".to_string());
    }

    if chain.fee_denom.is_empty() {
        return Err("fee_denom is required".to_string());
    }

    if chain.address_prefix.is_empty() {
        return Err("address_prefix is required".to_string());
    }

    if chain.gas_adjustment <= 0.0 {
        return Err("gas_adjustment must be greater than 0".to_string());
    }

    if chain.timeout_ms == 0 {
        return Err("timeout_ms must be greater than 0".to_string());
    }

    Ok(())
}

/// Validate a URL
pub fn validate_url(url: &str) -> std::result::Result<(), String> {
    if url.is_empty() {
        return Err("URL cannot be empty".to_string());
    }

    // Basic URL validation - check for scheme
    if !url.starts_with("http://")
        && !url.starts_with("https://")
        && !url.starts_with("ws://")
        && !url.starts_with("wss://")
    {
        return Err("URL must start with http://, https://, ws://, or wss://".to_string());
    }

    Ok(())
}

/// Validate log level
fn validate_log_level(level: &str) -> std::result::Result<(), ValidationError> {
    match level.to_lowercase().as_str() {
        "trace" | "debug" | "info" | "warn" | "error" => Ok(()),
        _ => Err(ValidationError::new(
            "network.log_level",
            format!(
                "invalid log level '{level}', must be one of: trace, debug, info, warn, error"
            ),
        )),
    }
}

/// Validate URLs across the entire configuration
pub fn validate_urls(config: &AppConfig) -> Result<()> {
    let mut errors = Vec::new();

    // Validate chain URLs
    for (chain_name, chain) in &config.chains {
        if let Err(e) = validate_url(&chain.rpc_url) {
            errors.push(ValidationError::new(
                format!("chains.{chain_name}.rpc_url"),
                e,
            ));
        }

        if let Some(grpc_url) = &chain.grpc_url {
            if let Err(e) = validate_url(grpc_url) {
                errors.push(ValidationError::new(
                    format!("chains.{chain_name}.grpc_url"),
                    e,
                ));
            }
        }
    }

    // Validate solver endpoints
    for (solver_id, endpoint) in &config.solvers.solver_endpoints {
        if let Err(e) = validate_url(endpoint) {
            errors.push(ValidationError::new(
                format!("solvers.solver_endpoints.{solver_id}"),
                e,
            ));
        }
    }

    // Validate oracle URLs
    if let Err(e) = validate_url(&config.oracle.endpoint) {
        errors.push(ValidationError::new("oracle.endpoint", e));
    }

    for (idx, fallback) in config.oracle.fallback_endpoints.iter().enumerate() {
        if let Err(e) = validate_url(fallback) {
            errors.push(ValidationError::new(
                format!("oracle.fallback_endpoints[{idx}]"),
                e,
            ));
        }
    }

    if !errors.is_empty() {
        let error_msg = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ConfigError::ValidationError(error_msg));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppConfig, ChainConfig, Environment, FeeConfig, NetworkConfig, OracleConfig, RelayerConfig,
        SettlementConfig, SolverConfig,
    };
    use std::collections::HashMap;

    #[test]
    fn test_validate_valid_config() {
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
            network: NetworkConfig {
                environment: Environment::Testnet,
                log_level: "info".to_string(),
                metrics_enabled: true,
                metrics_port: 9090,
            },
            chains,
            solvers: SolverConfig {
                enabled_solvers: vec!["solver1".to_string()],
                solver_endpoints,
                ..Default::default()
            },
            settlement: SettlementConfig {
                contract_address: "cosmos1abc".to_string(),
                ..Default::default()
            },
            oracle: OracleConfig {
                endpoint: "http://localhost:8080".to_string(),
                ..Default::default()
            },
            relayer: RelayerConfig::default(),
            fees: FeeConfig {
                fee_recipient: "cosmos1fee".to_string(),
                ..Default::default()
            },
        };

        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_invalid_log_level() {
        let config = AppConfig {
            network: NetworkConfig {
                log_level: "invalid".to_string(),
                ..Default::default()
            },
            solvers: SolverConfig {
                enabled_solvers: vec!["solver1".to_string()],
                ..Default::default()
            },
            settlement: SettlementConfig {
                contract_address: "cosmos1abc".to_string(),
                ..Default::default()
            },
            oracle: OracleConfig {
                endpoint: "http://localhost:8080".to_string(),
                ..Default::default()
            },
            fees: FeeConfig {
                fee_recipient: "cosmos1fee".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_validate_chain_config() {
        let valid_chain = ChainConfig {
            chain_id: "cosmoshub-4".to_string(),
            rpc_url: "https://rpc.cosmos.network".to_string(),
            grpc_url: None,
            gas_price: "0.025uatom".to_string(),
            fee_denom: "uatom".to_string(),
            address_prefix: "cosmos".to_string(),
            gas_adjustment: 1.3,
            timeout_ms: 30000,
            max_retries: 3,
        };

        assert!(validate_chain_config(&valid_chain).is_ok());

        let invalid_chain = ChainConfig {
            chain_id: "".to_string(),
            ..valid_chain.clone()
        };

        assert!(validate_chain_config(&invalid_chain).is_err());
    }

    #[test]
    fn test_validate_url() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://localhost:8080").is_ok());
        assert!(validate_url("ws://localhost:8080").is_ok());
        assert!(validate_url("wss://example.com").is_ok());

        assert!(validate_url("").is_err());
        assert!(validate_url("not-a-url").is_err());
        assert!(validate_url("ftp://example.com").is_err());
    }

    #[test]
    fn test_validate_no_enabled_solvers() {
        let config = AppConfig {
            solvers: SolverConfig {
                enabled_solvers: vec![],
                ..Default::default()
            },
            settlement: SettlementConfig {
                contract_address: "cosmos1abc".to_string(),
                ..Default::default()
            },
            oracle: OracleConfig {
                endpoint: "http://localhost:8080".to_string(),
                ..Default::default()
            },
            fees: FeeConfig {
                fee_recipient: "cosmos1fee".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(validate_config(&config).is_err());
    }
}
