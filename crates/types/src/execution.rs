use cosmwasm_schema::cw_serde;

/// User preference for asset delivery
#[cw_serde]
#[derive(Default)]
pub enum AssetPreference {
    /// Only accept canonical native denoms (uatom, Noble USDC, etc.)
    #[default]
    NativeOnly,

    /// Accept bridged representations from specific sources
    AcceptBridged { allowed_denoms: Vec<String> },

    /// Accept any fungible equivalent
    AnyEquivalent,
}

/// User preference for settlement path selection
#[cw_serde]
#[derive(Default)]
pub enum SettlementPreference {
    /// Prefer cheapest settlement path (default)
    #[default]
    Cost,
    /// Prefer fastest settlement path
    Latency,
}

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

    /// User preference for asset delivery
    pub asset_preference: AssetPreference,

    /// User preference for settlement path selection
    pub settlement_preference: SettlementPreference,

    /// Maximum settlement time constraint (seconds)
    pub max_settlement_secs: Option<u64>,
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
            asset_preference: AssetPreference::default(),
            settlement_preference: SettlementPreference::default(),
            max_settlement_secs: None,
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

    pub fn with_max_solver_fee_bps(mut self, bps: u32) -> Self {
        self.max_solver_fee_bps = Some(bps);
        self
    }

    pub fn with_max_bridge_time_secs(mut self, secs: u64) -> Self {
        self.max_bridge_time_secs = Some(secs);
        self
    }

    pub fn with_asset_preference(mut self, pref: AssetPreference) -> Self {
        self.asset_preference = pref;
        self
    }

    pub fn with_settlement_preference(mut self, pref: SettlementPreference) -> Self {
        self.settlement_preference = pref;
        self
    }

    pub fn with_max_settlement_secs(mut self, secs: u64) -> Self {
        self.max_settlement_secs = Some(secs);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_preference_native_only_default() {
        let constraints = ExecutionConstraints::default();
        assert!(matches!(constraints.asset_preference, AssetPreference::NativeOnly));
    }

    #[test]
    fn test_asset_preference_accept_bridged() {
        let constraints = ExecutionConstraints::new(1000)
            .with_asset_preference(AssetPreference::AcceptBridged {
                allowed_denoms: vec!["eureka/usdc".to_string()],
            });

        match &constraints.asset_preference {
            AssetPreference::AcceptBridged { allowed_denoms } => {
                assert!(allowed_denoms.contains(&"eureka/usdc".to_string()));
            }
            _ => panic!("Expected AcceptBridged"),
        }
    }

    #[test]
    fn test_settlement_preference_default_cost() {
        let constraints = ExecutionConstraints::default();
        assert!(matches!(constraints.settlement_preference, SettlementPreference::Cost));
    }

    #[test]
    fn test_settlement_preference_latency() {
        let constraints = ExecutionConstraints::new(1000)
            .with_settlement_preference(SettlementPreference::Latency);
        assert!(matches!(
            constraints.settlement_preference,
            SettlementPreference::Latency
        ));
    }

    #[test]
    fn test_max_settlement_secs() {
        let constraints = ExecutionConstraints::new(1000).with_max_settlement_secs(3600);
        assert_eq!(constraints.max_settlement_secs, Some(3600));
    }
}
