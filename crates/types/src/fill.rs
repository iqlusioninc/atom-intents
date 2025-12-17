use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

/// Configuration for partial fills
#[cw_serde]
pub struct FillConfig {
    /// Allow partial fills?
    pub allow_partial: bool,

    /// Minimum fill amount (absolute)
    pub min_fill_amount: Uint128,

    /// Minimum fill percentage (0.0 - 1.0) as string
    pub min_fill_pct: String,

    /// Time window to aggregate fills (ms)
    pub aggregation_window_ms: u64,

    /// Fill strategy
    pub strategy: FillStrategy,
}

impl Default for FillConfig {
    fn default() -> Self {
        Self {
            allow_partial: true,
            min_fill_amount: Uint128::zero(),
            min_fill_pct: "0.1".to_string(), // 10%
            aggregation_window_ms: 5000,      // 5 seconds
            strategy: FillStrategy::Eager,
        }
    }
}

#[cw_serde]
pub enum FillStrategy {
    /// Accept any fills meeting price
    Eager,

    /// Full fill or nothing
    AllOrNothing,

    /// Require minimum %, then accept any additional
    MinimumThenEager { min_pct: String },

    /// Let solver optimize
    SolverDiscretion,
}

/// A completed or proposed fill
#[cw_serde]
pub struct Fill {
    /// Amount of input consumed
    pub input_amount: Uint128,

    /// Amount of output provided
    pub output_amount: Uint128,

    /// Effective price (output/input)
    pub price: String,

    /// Source of this fill
    pub source: FillSource,
}

#[cw_serde]
pub enum FillSource {
    /// Direct crossing with another intent
    IntentMatch { counterparty: String },

    /// Routed through DEX
    DexRoute { venue: String, steps: Vec<String> },

    /// Filled from solver inventory
    SolverInventory { solver_id: String },

    /// Hedged against CEX
    CexHedge { exchange: String },
}

/// Proposed fill from a solver
#[cw_serde]
pub struct ProposedFill {
    pub input_amount: Uint128,
    pub output_amount: Uint128,
    pub price: String,
}
