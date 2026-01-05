use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

use crate::{Ecosystem, EurekaDirection, ExecutionPlan, ProposedFill, SettlementPath};

/// Metadata about a solution's settlement characteristics
#[cw_serde]
pub struct SolutionMetadata {
    /// Which ecosystem the liquidity is sourced from
    pub ecosystem: Ecosystem,

    /// How the settlement will occur
    pub settlement_path: SettlementPath,

    /// Estimated settlement time in seconds
    pub estimated_time_secs: u64,

    /// Estimated settlement cost in USD
    pub estimated_cost_usd: String,

    /// Actual denomination that will be delivered
    pub output_denom: String,

    /// Whether output is native (true) or bridged (false)
    pub is_native_output: bool,
}

impl SolutionMetadata {
    /// Create metadata for a Cosmos IBC settlement
    pub fn cosmos_ibc(channel: &str, output_denom: &str, is_multi_hop: bool) -> Self {
        Self {
            ecosystem: Ecosystem::Cosmos,
            settlement_path: SettlementPath::CosmosIbc {
                channel: channel.to_string(),
                is_multi_hop,
            },
            estimated_time_secs: if is_multi_hop { 20 } else { 6 },
            estimated_cost_usd: if is_multi_hop {
                "0.02".to_string()
            } else {
                "0.01".to_string()
            },
            output_denom: output_denom.to_string(),
            is_native_output: true,
        }
    }

    /// Create metadata for an Eureka settlement
    pub fn eureka(
        direction: EurekaDirection,
        eth_address: Option<String>,
        output_denom: &str,
        is_native: bool,
    ) -> Self {
        Self {
            ecosystem: Ecosystem::Ethereum,
            settlement_path: SettlementPath::Eureka {
                direction,
                eth_address,
            },
            estimated_time_secs: 25,
            estimated_cost_usd: "3.00".to_string(),
            output_denom: output_denom.to_string(),
            is_native_output: is_native,
        }
    }
}

/// A solver's proposed solution for an intent
#[cw_serde]
pub struct Solution {
    /// Solver identifier
    pub solver_id: String,

    /// Intent being solved
    pub intent_id: String,

    /// Proposed fill details
    pub fill: ProposedFill,

    /// How the fill will be executed
    pub execution: ExecutionPlan,

    /// Solution validity deadline (Unix timestamp)
    pub valid_until: u64,

    /// Bond amount committed by solver
    pub bond: Uint128,

    /// Cross-ecosystem metadata (optional for backwards compatibility)
    pub metadata: Option<SolutionMetadata>,
}

/// Context provided to solvers when solving
#[cw_serde]
pub struct SolveContext {
    /// Amount already matched via intent crossing
    pub matched_amount: Uint128,

    /// Remaining amount to fill
    pub remaining: Uint128,

    /// Current oracle price for reference
    pub oracle_price: String,
}

/// Solver capabilities declaration
#[cw_serde]
pub struct SolverCapabilities {
    /// Can route through DEXes
    pub dex_routing: bool,

    /// Can match against other intents
    pub intent_matching: bool,

    /// Has CEX backstop capability
    pub cex_backstop: bool,

    /// Can execute cross-ecosystem
    pub cross_ecosystem: bool,

    /// Maximum fill size in USD
    pub max_fill_size_usd: u64,
}

impl Default for SolverCapabilities {
    fn default() -> Self {
        Self {
            dex_routing: true,
            intent_matching: false,
            cex_backstop: false,
            cross_ecosystem: false,
            max_fill_size_usd: 100_000,
        }
    }
}

/// Solver capacity for a specific pair
#[cw_serde]
pub struct SolverCapacity {
    /// Maximum input amount for immediate fill
    pub max_immediate: Uint128,

    /// Current available liquidity
    pub available_liquidity: Uint128,

    /// Estimated execution time (ms)
    pub estimated_time_ms: u64,
}

/// Quote from a solver for an intent
#[cw_serde]
pub struct SolverQuote {
    pub solver_id: String,
    pub input_amount: Uint128,
    pub output_amount: Uint128,
    pub price: String,
    pub valid_for_ms: u64,
}

/// Result of aggregating multiple solutions
pub struct OptimalFillPlan {
    /// Selected solutions with amounts
    pub selected: Vec<(Solution, Uint128)>,

    /// Total input amount covered
    pub total_input: Uint128,
}

impl OptimalFillPlan {
    pub fn fully_matched(_intent_id: &str, amount: Uint128) -> Self {
        Self {
            selected: vec![],
            total_input: amount,
        }
    }
}

/// Solver registration information
#[cw_serde]
pub struct SolverInfo {
    pub id: String,
    pub name: String,
    pub operator: String,
    pub capabilities: SolverCapabilities,
    pub bond_amount: Uint128,
    pub registered_at: u64,
    pub active: bool,
}

/// Solver performance statistics
#[cw_serde]
pub struct SolverStats {
    pub solver_id: String,
    pub total_fills: u64,
    pub successful_fills: u64,
    pub failed_fills: u64,
    pub total_volume_usd: u64,
    pub average_fill_time_ms: u64,
    pub slashing_events: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ecosystem, EurekaDirection, SettlementPath};

    #[test]
    fn test_solution_metadata_cosmos() {
        let metadata = SolutionMetadata {
            ecosystem: Ecosystem::Cosmos,
            settlement_path: SettlementPath::CosmosIbc {
                channel: "channel-141".to_string(),
                is_multi_hop: false,
            },
            estimated_time_secs: 6,
            estimated_cost_usd: "0.01".to_string(),
            output_denom: "uusdc".to_string(),
            is_native_output: true,
        };

        assert_eq!(metadata.ecosystem, Ecosystem::Cosmos);
        assert!(metadata.is_native_output);
    }

    #[test]
    fn test_solution_metadata_eureka() {
        let metadata = SolutionMetadata {
            ecosystem: Ecosystem::Ethereum,
            settlement_path: SettlementPath::Eureka {
                direction: EurekaDirection::EthereumToCosmos,
                eth_address: Some("0x123".to_string()),
            },
            estimated_time_secs: 25,
            estimated_cost_usd: "3.00".to_string(),
            output_denom: "eureka/usdc".to_string(),
            is_native_output: false,
        };

        assert_eq!(metadata.ecosystem, Ecosystem::Ethereum);
        assert!(!metadata.is_native_output);
    }

    #[test]
    fn test_solution_metadata_cosmos_ibc_helper() {
        let metadata = SolutionMetadata::cosmos_ibc("channel-141", "uusdc", false);

        assert_eq!(metadata.ecosystem, Ecosystem::Cosmos);
        assert_eq!(metadata.output_denom, "uusdc");
        assert_eq!(metadata.estimated_time_secs, 6);
        assert_eq!(metadata.estimated_cost_usd, "0.01");
        assert!(metadata.is_native_output);

        // Test multi-hop settings
        let multi_hop = SolutionMetadata::cosmos_ibc("channel-0", "uatom", true);
        assert_eq!(multi_hop.estimated_time_secs, 20);
        assert_eq!(multi_hop.estimated_cost_usd, "0.02");
    }

    #[test]
    fn test_solution_metadata_eureka_helper() {
        let metadata = SolutionMetadata::eureka(
            EurekaDirection::EthereumToCosmos,
            Some("0xabc123".to_string()),
            "eureka/usdc",
            false,
        );

        assert_eq!(metadata.ecosystem, Ecosystem::Ethereum);
        assert_eq!(metadata.output_denom, "eureka/usdc");
        assert_eq!(metadata.estimated_time_secs, 25);
        assert_eq!(metadata.estimated_cost_usd, "3.00");
        assert!(!metadata.is_native_output);

        // Test native output
        let native = SolutionMetadata::eureka(
            EurekaDirection::CosmosToEthereum,
            None,
            "usdc",
            true,
        );
        assert!(native.is_native_output);
    }
}
