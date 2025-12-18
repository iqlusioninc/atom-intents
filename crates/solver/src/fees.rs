//! Transaction fee estimation framework for ATOM Intent-Based Liquidity System
//!
//! This module provides comprehensive fee estimation for various blockchain operations
//! including swaps, IBC transfers, escrow operations, and multi-hop routes across
//! Cosmos chains.
//!
//! # Features
//!
//! - Pre-configured fee parameters for major Cosmos chains
//! - Dynamic gas price updates with caching
//! - Operation-specific gas cost models
//! - Multi-hop route fee calculation
//! - Priority-based fee selection (Low, Medium, High)
//!
//! # Example
//!
//! ```
//! use atom_intents_solver::{FeeEstimator, SettlementFlowType, FeePriority};
//!
//! let estimator = FeeEstimator::new();
//!
//! // Estimate a simple swap on Osmosis
//! let fee = estimator.estimate_settlement_fee(
//!     SettlementFlowType::SimpleSwap,
//!     "osmosis-1",
//!     FeePriority::Medium,
//! ).unwrap();
//!
//! println!("Gas limit: {}", fee.gas_limit);
//! println!("Fee: {} {}", fee.fee_amount, fee.fee_denom);
//!
//! // Estimate multi-hop IBC transfer
//! let route = vec!["cosmoshub-4", "osmosis-1", "neutron-1"];
//! let total = estimator.total_multi_hop_fee(&route, FeePriority::High).unwrap();
//! println!("Total multi-hop fee: {} {}", total.fee_amount, total.fee_denom);
//! ```

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FeeError {
    #[error("chain not configured: {0}")]
    ChainNotConfigured(String),

    #[error("invalid gas price")]
    InvalidGasPrice,

    #[error("failed to query fee market: {0}")]
    QueryFailed(String),

    #[error("stale gas price data")]
    StaleData,
}

/// Fee estimator with chain configs and dynamic gas prices
pub struct FeeEstimator {
    chain_configs: HashMap<String, ChainFeeConfig>,
    gas_prices: HashMap<String, GasPrice>,
    gas_costs: GasCosts,
}

/// Configuration for a specific chain's fee parameters
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainFeeConfig {
    pub chain_id: String,
    pub fee_denom: String,
    pub min_gas_price: Decimal,
    pub avg_gas_price: Decimal,
    pub high_gas_price: Decimal,
    /// Whether chain supports EIP-1559 style fee market
    pub supports_eip1559: bool,
}

/// Current gas prices for a chain
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GasPrice {
    pub low: Decimal,
    pub average: Decimal,
    pub high: Decimal,
    pub updated_at: u64,
}

/// Complete fee estimate for an operation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeeEstimate {
    pub gas_limit: u64,
    pub fee_amount: u128,
    pub fee_denom: String,
    pub priority: FeePriority,
}

/// Priority level for fee selection
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum FeePriority {
    Low,
    Medium,
    High,
}

/// Gas cost models for different operations
#[derive(Clone, Debug)]
pub struct GasCosts {
    pub ibc_transfer: u64,
    pub swap: u64,
    pub escrow_lock: u64,
    pub escrow_release: u64,
    pub multi_hop_per_hop: u64,
}

impl Default for GasCosts {
    fn default() -> Self {
        Self {
            ibc_transfer: 150_000,
            swap: 300_000,
            escrow_lock: 100_000,
            escrow_release: 80_000,
            multi_hop_per_hop: 100_000,
        }
    }
}

impl FeeEstimator {
    /// Create a new fee estimator with pre-configured chains
    pub fn new() -> Self {
        let mut chain_configs = HashMap::new();

        // Cosmos Hub
        chain_configs.insert(
            "cosmoshub-4".to_string(),
            ChainFeeConfig {
                chain_id: "cosmoshub-4".to_string(),
                fee_denom: "uatom".to_string(),
                min_gas_price: Decimal::from_str("0.01").unwrap(),
                avg_gas_price: Decimal::from_str("0.025").unwrap(),
                high_gas_price: Decimal::from_str("0.05").unwrap(),
                supports_eip1559: false,
            },
        );

        // Osmosis
        chain_configs.insert(
            "osmosis-1".to_string(),
            ChainFeeConfig {
                chain_id: "osmosis-1".to_string(),
                fee_denom: "uosmo".to_string(),
                min_gas_price: Decimal::from_str("0.0025").unwrap(),
                avg_gas_price: Decimal::from_str("0.025").unwrap(),
                high_gas_price: Decimal::from_str("0.04").unwrap(),
                supports_eip1559: false,
            },
        );

        // Neutron
        chain_configs.insert(
            "neutron-1".to_string(),
            ChainFeeConfig {
                chain_id: "neutron-1".to_string(),
                fee_denom: "untrn".to_string(),
                min_gas_price: Decimal::from_str("0.005").unwrap(),
                avg_gas_price: Decimal::from_str("0.01").unwrap(),
                high_gas_price: Decimal::from_str("0.02").unwrap(),
                supports_eip1559: false,
            },
        );

        // Stride
        chain_configs.insert(
            "stride-1".to_string(),
            ChainFeeConfig {
                chain_id: "stride-1".to_string(),
                fee_denom: "ustrd".to_string(),
                min_gas_price: Decimal::from_str("0.001").unwrap(),
                avg_gas_price: Decimal::from_str("0.005").unwrap(),
                high_gas_price: Decimal::from_str("0.01").unwrap(),
                supports_eip1559: false,
            },
        );

        // Juno
        chain_configs.insert(
            "juno-1".to_string(),
            ChainFeeConfig {
                chain_id: "juno-1".to_string(),
                fee_denom: "ujuno".to_string(),
                min_gas_price: Decimal::from_str("0.0025").unwrap(),
                avg_gas_price: Decimal::from_str("0.025").unwrap(),
                high_gas_price: Decimal::from_str("0.04").unwrap(),
                supports_eip1559: false,
            },
        );

        // Stargaze
        chain_configs.insert(
            "stargaze-1".to_string(),
            ChainFeeConfig {
                chain_id: "stargaze-1".to_string(),
                fee_denom: "ustars".to_string(),
                min_gas_price: Decimal::from_str("0.5").unwrap(),
                avg_gas_price: Decimal::from_str("1.0").unwrap(),
                high_gas_price: Decimal::from_str("2.0").unwrap(),
                supports_eip1559: false,
            },
        );

        // Injective
        chain_configs.insert(
            "injective-1".to_string(),
            ChainFeeConfig {
                chain_id: "injective-1".to_string(),
                fee_denom: "inj".to_string(),
                min_gas_price: Decimal::from_str("160000000").unwrap(),
                avg_gas_price: Decimal::from_str("500000000").unwrap(),
                high_gas_price: Decimal::from_str("1000000000").unwrap(),
                supports_eip1559: false,
            },
        );

        // Celestia
        chain_configs.insert(
            "celestia".to_string(),
            ChainFeeConfig {
                chain_id: "celestia".to_string(),
                fee_denom: "utia".to_string(),
                min_gas_price: Decimal::from_str("0.01").unwrap(),
                avg_gas_price: Decimal::from_str("0.02").unwrap(),
                high_gas_price: Decimal::from_str("0.05").unwrap(),
                supports_eip1559: false,
            },
        );

        Self {
            chain_configs,
            gas_prices: HashMap::new(),
            gas_costs: GasCosts::default(),
        }
    }

    /// Add or update a chain configuration
    pub fn add_chain_config(&mut self, config: ChainFeeConfig) {
        self.chain_configs.insert(config.chain_id.clone(), config);
    }

    /// Update gas prices for a chain
    pub fn update_gas_prices(&mut self, chain_id: &str, gas_price: GasPrice) {
        self.gas_prices.insert(chain_id.to_string(), gas_price);
    }

    /// Get chain config
    pub fn get_chain_config(&self, chain_id: &str) -> Result<&ChainFeeConfig, FeeError> {
        self.chain_configs
            .get(chain_id)
            .ok_or_else(|| FeeError::ChainNotConfigured(chain_id.to_string()))
    }

    /// Estimate settlement fee for a complete flow
    pub fn estimate_settlement_fee(
        &self,
        flow_type: SettlementFlowType,
        chain_id: &str,
        priority: FeePriority,
    ) -> Result<FeeEstimate, FeeError> {
        let config = self.get_chain_config(chain_id)?;

        let gas_limit = match flow_type {
            SettlementFlowType::SimpleSwap => self.gas_costs.swap,
            SettlementFlowType::IbcTransfer => self.gas_costs.ibc_transfer,
            SettlementFlowType::EscrowLock => self.gas_costs.escrow_lock,
            SettlementFlowType::EscrowRelease => self.gas_costs.escrow_release,
            SettlementFlowType::SwapAndTransfer => {
                self.gas_costs.swap + self.gas_costs.ibc_transfer
            }
            SettlementFlowType::MultiHop { hops } => {
                self.gas_costs.swap + (self.gas_costs.multi_hop_per_hop * hops as u64)
            }
        };

        let gas_price = self.get_gas_price(chain_id, &priority, config)?;
        let fee_amount = self.calculate_fee(gas_limit, gas_price);

        Ok(FeeEstimate {
            gas_limit,
            fee_amount,
            fee_denom: config.fee_denom.clone(),
            priority,
        })
    }

    /// Estimate IBC transfer fee
    pub fn estimate_ibc_fee(
        &self,
        source_chain: &str,
        dest_chain: &str,
        priority: FeePriority,
    ) -> Result<FeeEstimate, FeeError> {
        // Fee is paid on source chain
        let config = self.get_chain_config(source_chain)?;
        let gas_limit = self.gas_costs.ibc_transfer;

        let gas_price = self.get_gas_price(source_chain, &priority, config)?;
        let fee_amount = self.calculate_fee(gas_limit, gas_price);

        Ok(FeeEstimate {
            gas_limit,
            fee_amount,
            fee_denom: config.fee_denom.clone(),
            priority,
        })
    }

    /// Estimate swap fee on a specific chain
    pub fn estimate_swap_fee(
        &self,
        chain_id: &str,
        num_hops: u32,
        priority: FeePriority,
    ) -> Result<FeeEstimate, FeeError> {
        let config = self.get_chain_config(chain_id)?;

        let gas_limit = if num_hops <= 1 {
            self.gas_costs.swap
        } else {
            self.gas_costs.swap + (self.gas_costs.multi_hop_per_hop * (num_hops - 1) as u64)
        };

        let gas_price = self.get_gas_price(chain_id, &priority, config)?;
        let fee_amount = self.calculate_fee(gas_limit, gas_price);

        Ok(FeeEstimate {
            gas_limit,
            fee_amount,
            fee_denom: config.fee_denom.clone(),
            priority,
        })
    }

    /// Estimate multi-hop PFM (Packet Forward Middleware) fee
    pub fn estimate_multi_hop_fee(
        &self,
        route: &[&str],
        priority: FeePriority,
    ) -> Result<Vec<FeeEstimate>, FeeError> {
        if route.is_empty() {
            return Ok(vec![]);
        }

        let mut estimates = Vec::new();

        // First hop includes the initial transfer
        let first_estimate = self.estimate_ibc_fee(route[0], route[1], priority.clone())?;
        estimates.push(first_estimate);

        // Subsequent hops
        for i in 1..route.len() - 1 {
            let hop_estimate = self.estimate_ibc_fee(route[i], route[i + 1], priority.clone())?;
            estimates.push(hop_estimate);
        }

        Ok(estimates)
    }

    /// Calculate total fee for multi-hop route
    pub fn total_multi_hop_fee(
        &self,
        route: &[&str],
        priority: FeePriority,
    ) -> Result<FeeEstimate, FeeError> {
        let estimates = self.estimate_multi_hop_fee(route, priority.clone())?;

        if estimates.is_empty() {
            return Err(FeeError::InvalidGasPrice);
        }

        let total_gas = estimates.iter().map(|e| e.gas_limit).sum();
        let total_fee = estimates.iter().map(|e| e.fee_amount).sum();

        // Use first chain's denom
        let fee_denom = estimates[0].fee_denom.clone();

        Ok(FeeEstimate {
            gas_limit: total_gas,
            fee_amount: total_fee,
            fee_denom,
            priority,
        })
    }

    /// Get gas price based on priority
    fn get_gas_price(
        &self,
        chain_id: &str,
        priority: &FeePriority,
        config: &ChainFeeConfig,
    ) -> Result<Decimal, FeeError> {
        // Check if we have recent gas price data
        if let Some(gas_price) = self.gas_prices.get(chain_id) {
            // Check if data is fresh (within 5 minutes)
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            if current_time - gas_price.updated_at < 300 {
                return Ok(match priority {
                    FeePriority::Low => gas_price.low,
                    FeePriority::Medium => gas_price.average,
                    FeePriority::High => gas_price.high,
                });
            }
        }

        // Fall back to configured defaults
        Ok(match priority {
            FeePriority::Low => config.min_gas_price,
            FeePriority::Medium => config.avg_gas_price,
            FeePriority::High => config.high_gas_price,
        })
    }

    /// Calculate fee from gas limit and gas price
    fn calculate_fee(&self, gas_limit: u64, gas_price: Decimal) -> u128 {
        let gas_decimal = Decimal::from(gas_limit);
        let fee = gas_decimal * gas_price;

        // Convert to u128, rounding up
        fee.ceil().to_string().parse::<u128>().unwrap_or(0)
    }

    /// Query current gas prices from chain (mock implementation)
    /// In production, this would query the chain's fee market
    pub async fn query_gas_prices(&self, chain_id: &str) -> Result<GasPrice, FeeError> {
        let config = self.get_chain_config(chain_id)?;

        // TODO: Implement actual chain query via RPC
        // For now, return configured defaults with current timestamp
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(GasPrice {
            low: config.min_gas_price,
            average: config.avg_gas_price,
            high: config.high_gas_price,
            updated_at: current_time,
        })
    }

    /// Update all gas prices (cache refresh)
    pub async fn refresh_gas_prices(&mut self) -> Result<(), FeeError> {
        let chain_ids: Vec<String> = self.chain_configs.keys().cloned().collect();

        for chain_id in chain_ids {
            if let Ok(gas_price) = self.query_gas_prices(&chain_id).await {
                self.update_gas_prices(&chain_id, gas_price);
            }
        }

        Ok(())
    }

    /// Check if gas price data is stale
    pub fn is_stale(&self, chain_id: &str, max_age_seconds: u64) -> bool {
        if let Some(gas_price) = self.gas_prices.get(chain_id) {
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            current_time - gas_price.updated_at > max_age_seconds
        } else {
            true
        }
    }
}

impl Default for FeeEstimator {
    fn default() -> Self {
        Self::new()
    }
}

/// Types of settlement flows
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettlementFlowType {
    SimpleSwap,
    IbcTransfer,
    EscrowLock,
    EscrowRelease,
    SwapAndTransfer,
    MultiHop { hops: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_estimator_creation() {
        let estimator = FeeEstimator::new();

        // Check that known chains are configured
        assert!(estimator.get_chain_config("cosmoshub-4").is_ok());
        assert!(estimator.get_chain_config("osmosis-1").is_ok());
        assert!(estimator.get_chain_config("neutron-1").is_ok());
    }

    #[test]
    fn test_simple_swap_fee() {
        let estimator = FeeEstimator::new();

        let fee = estimator
            .estimate_settlement_fee(
                SettlementFlowType::SimpleSwap,
                "osmosis-1",
                FeePriority::Medium,
            )
            .unwrap();

        assert_eq!(fee.gas_limit, 300_000);
        assert_eq!(fee.fee_denom, "uosmo");
        assert!(fee.fee_amount > 0);
        assert_eq!(fee.priority, FeePriority::Medium);
    }

    #[test]
    fn test_ibc_transfer_fee() {
        let estimator = FeeEstimator::new();

        let fee = estimator
            .estimate_ibc_fee("cosmoshub-4", "osmosis-1", FeePriority::High)
            .unwrap();

        assert_eq!(fee.gas_limit, 150_000);
        assert_eq!(fee.fee_denom, "uatom");
        assert!(fee.fee_amount > 0);
        assert_eq!(fee.priority, FeePriority::High);
    }

    #[test]
    fn test_multi_hop_swap_fee() {
        let estimator = FeeEstimator::new();

        let fee = estimator
            .estimate_swap_fee("osmosis-1", 3, FeePriority::Low)
            .unwrap();

        // Should be: 300_000 (base swap) + 2 * 100_000 (additional hops)
        assert_eq!(fee.gas_limit, 500_000);
        assert_eq!(fee.fee_denom, "uosmo");
    }

    #[test]
    fn test_swap_and_transfer_fee() {
        let estimator = FeeEstimator::new();

        let fee = estimator
            .estimate_settlement_fee(
                SettlementFlowType::SwapAndTransfer,
                "osmosis-1",
                FeePriority::Medium,
            )
            .unwrap();

        // Should be: 300_000 (swap) + 150_000 (IBC transfer)
        assert_eq!(fee.gas_limit, 450_000);
    }

    #[test]
    fn test_multi_hop_route_fee() {
        let estimator = FeeEstimator::new();

        let route = vec!["cosmoshub-4", "osmosis-1", "neutron-1"];
        let estimates = estimator
            .estimate_multi_hop_fee(&route, FeePriority::Medium)
            .unwrap();

        // Should have 2 fee estimates (2 hops)
        assert_eq!(estimates.len(), 2);
        assert_eq!(estimates[0].fee_denom, "uatom"); // First hop from cosmoshub
        assert_eq!(estimates[1].fee_denom, "uosmo"); // Second hop from osmosis
    }

    #[test]
    fn test_total_multi_hop_fee() {
        let estimator = FeeEstimator::new();

        let route = vec!["cosmoshub-4", "osmosis-1", "neutron-1"];
        let total = estimator
            .total_multi_hop_fee(&route, FeePriority::Medium)
            .unwrap();

        // Total gas should be 2 * 150_000
        assert_eq!(total.gas_limit, 300_000);
        assert!(total.fee_amount > 0);
    }

    #[test]
    fn test_fee_priority_levels() {
        let estimator = FeeEstimator::new();

        let low = estimator
            .estimate_settlement_fee(
                SettlementFlowType::SimpleSwap,
                "osmosis-1",
                FeePriority::Low,
            )
            .unwrap();

        let medium = estimator
            .estimate_settlement_fee(
                SettlementFlowType::SimpleSwap,
                "osmosis-1",
                FeePriority::Medium,
            )
            .unwrap();

        let high = estimator
            .estimate_settlement_fee(
                SettlementFlowType::SimpleSwap,
                "osmosis-1",
                FeePriority::High,
            )
            .unwrap();

        // Higher priority should have higher fees
        assert!(low.fee_amount < medium.fee_amount);
        assert!(medium.fee_amount < high.fee_amount);
    }

    #[test]
    fn test_escrow_operations() {
        let estimator = FeeEstimator::new();

        let lock = estimator
            .estimate_settlement_fee(
                SettlementFlowType::EscrowLock,
                "cosmoshub-4",
                FeePriority::Medium,
            )
            .unwrap();

        let release = estimator
            .estimate_settlement_fee(
                SettlementFlowType::EscrowRelease,
                "cosmoshub-4",
                FeePriority::Medium,
            )
            .unwrap();

        assert_eq!(lock.gas_limit, 100_000);
        assert_eq!(release.gas_limit, 80_000);
        assert!(release.fee_amount < lock.fee_amount);
    }

    #[test]
    fn test_unknown_chain_error() {
        let estimator = FeeEstimator::new();

        let result = estimator.estimate_settlement_fee(
            SettlementFlowType::SimpleSwap,
            "unknown-chain",
            FeePriority::Medium,
        );

        assert!(result.is_err());
        match result {
            Err(FeeError::ChainNotConfigured(chain)) => {
                assert_eq!(chain, "unknown-chain");
            }
            _ => panic!("Expected ChainNotConfigured error"),
        }
    }

    #[test]
    fn test_add_custom_chain_config() {
        let mut estimator = FeeEstimator::new();

        let custom_config = ChainFeeConfig {
            chain_id: "custom-1".to_string(),
            fee_denom: "ucustom".to_string(),
            min_gas_price: Decimal::from_str("0.001").unwrap(),
            avg_gas_price: Decimal::from_str("0.01").unwrap(),
            high_gas_price: Decimal::from_str("0.1").unwrap(),
            supports_eip1559: false,
        };

        estimator.add_chain_config(custom_config);

        let fee = estimator
            .estimate_settlement_fee(
                SettlementFlowType::SimpleSwap,
                "custom-1",
                FeePriority::Medium,
            )
            .unwrap();

        assert_eq!(fee.fee_denom, "ucustom");
    }

    #[test]
    fn test_gas_price_update() {
        let mut estimator = FeeEstimator::new();

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let new_gas_price = GasPrice {
            low: Decimal::from_str("0.001").unwrap(),
            average: Decimal::from_str("0.01").unwrap(),
            high: Decimal::from_str("0.1").unwrap(),
            updated_at: current_time,
        };

        estimator.update_gas_prices("osmosis-1", new_gas_price);

        let fee = estimator
            .estimate_settlement_fee(
                SettlementFlowType::SimpleSwap,
                "osmosis-1",
                FeePriority::Medium,
            )
            .unwrap();

        // Should use the updated gas price
        let expected_fee = 300_000u128 * 10_000_000_000u128 / 1_000_000_000_000u128; // 0.01 * 300_000
        assert_eq!(fee.fee_amount, 3000);
    }

    #[test]
    fn test_gas_price_staleness() {
        let mut estimator = FeeEstimator::new();

        // Add old gas price
        let old_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 600; // 10 minutes ago

        let old_gas_price = GasPrice {
            low: Decimal::from_str("0.001").unwrap(),
            average: Decimal::from_str("0.01").unwrap(),
            high: Decimal::from_str("0.1").unwrap(),
            updated_at: old_time,
        };

        estimator.update_gas_prices("osmosis-1", old_gas_price);

        // Should be stale (older than 5 minutes)
        assert!(estimator.is_stale("osmosis-1", 300));

        // Should not be stale for 15 minute threshold
        assert!(!estimator.is_stale("osmosis-1", 900));
    }

    #[tokio::test]
    async fn test_query_gas_prices() {
        let estimator = FeeEstimator::new();

        let gas_price = estimator.query_gas_prices("cosmoshub-4").await.unwrap();

        assert!(gas_price.low > Decimal::ZERO);
        assert!(gas_price.average > gas_price.low);
        assert!(gas_price.high > gas_price.average);
        assert!(gas_price.updated_at > 0);
    }

    #[test]
    fn test_multi_hop_flow_type() {
        let estimator = FeeEstimator::new();

        let fee = estimator
            .estimate_settlement_fee(
                SettlementFlowType::MultiHop { hops: 4 },
                "osmosis-1",
                FeePriority::Medium,
            )
            .unwrap();

        // Should be: 300_000 (base swap) + 4 * 100_000 (hops)
        assert_eq!(fee.gas_limit, 700_000);
    }

    #[test]
    fn test_fee_calculation_precision() {
        let estimator = FeeEstimator::new();

        // Test with different gas prices to ensure precision
        let gas_limit = 100_000u64;
        let gas_price = Decimal::from_str("0.025").unwrap();

        let fee = estimator.calculate_fee(gas_limit, gas_price);

        // 100_000 * 0.025 = 2_500
        assert_eq!(fee, 2_500);
    }

    #[test]
    fn test_all_configured_chains() {
        let estimator = FeeEstimator::new();

        let chains = vec![
            "cosmoshub-4",
            "osmosis-1",
            "neutron-1",
            "stride-1",
            "juno-1",
            "stargaze-1",
            "injective-1",
            "celestia",
        ];

        for chain in chains {
            let config = estimator.get_chain_config(chain).unwrap();
            assert_eq!(config.chain_id, chain);
            assert!(!config.fee_denom.is_empty());
            assert!(config.min_gas_price > Decimal::ZERO);
            assert!(config.avg_gas_price >= config.min_gas_price);
            assert!(config.high_gas_price >= config.avg_gas_price);
        }
    }
}
