use cosmwasm_schema::cw_serde;

/// Constraints on how intent can be executed
#[cw_serde]
pub struct ExecutionConstraints {
    /// Absolute deadline (Unix timestamp in seconds)
    pub deadline: u64,

    /// Maximum IBC hops allowed
    pub max_hops: Option<u32>,

    /// Venues to exclude
    pub excluded_venues: Vec<String>,

    /// Maximum solver fee (basis points)
    pub max_solver_fee_bps: Option<u32>,

    /// Allow cross-ecosystem execution (NEAR, etc.)
    pub allow_cross_ecosystem: bool,

    /// Maximum bridge latency acceptable (seconds)
    pub max_bridge_time_secs: Option<u64>,
}

impl ExecutionConstraints {
    pub fn new(deadline: u64) -> Self {
        Self {
            deadline,
            max_hops: Some(3),
            excluded_venues: vec![],
            max_solver_fee_bps: Some(50), // 0.5% max
            allow_cross_ecosystem: false, // Cosmos-only by default
            max_bridge_time_secs: None,
        }
    }

    pub fn with_max_hops(mut self, hops: u32) -> Self {
        self.max_hops = Some(hops);
        self
    }

    pub fn with_cross_ecosystem(mut self, allow: bool) -> Self {
        self.allow_cross_ecosystem = allow;
        self
    }

    pub fn exclude_venue(mut self, venue: impl Into<String>) -> Self {
        self.excluded_venues.push(venue.into());
        self
    }
}

impl Default for ExecutionConstraints {
    fn default() -> Self {
        Self::new(0) // Caller should set appropriate deadline
    }
}

/// Execution plan describing how to fill an intent
#[cw_serde]
pub enum ExecutionPlan {
    /// Route through DEX(es)
    DexRoute { steps: Vec<DexSwapStep> },

    /// Fill from solver inventory
    InventoryFill { source_chain: String },

    /// Hedge against CEX
    CexHedge { exchange: String },

    /// Cross-ecosystem via bridge
    CrossEcosystem { bridge: String, target: String },
}

/// A single step in a DEX route
#[cw_serde]
pub struct DexSwapStep {
    /// DEX name (e.g., "osmosis", "astroport")
    pub venue: String,

    /// Pool ID or contract address
    pub pool_id: String,

    /// Input denomination
    pub input_denom: String,

    /// Output denomination
    pub output_denom: String,

    /// Chain where swap occurs
    pub chain_id: String,
}
