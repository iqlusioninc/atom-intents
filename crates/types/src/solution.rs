use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

use crate::{ExecutionPlan, ProposedFill};

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
    pub fn fully_matched(intent_id: &str, amount: Uint128) -> Self {
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
