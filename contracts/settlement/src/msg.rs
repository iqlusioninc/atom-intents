use atom_intents_types::collateral::{AssetClass, CollateralAsset, LiquidationStatus};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub escrow_contract: String,
    pub min_solver_bond: Uint128,
    pub base_slash_bps: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Register as a solver (requires bond)
    RegisterSolver { solver_id: String },

    /// Deregister solver (returns bond if no pending settlements)
    DeregisterSolver { solver_id: String },

    /// Create a new settlement
    CreateSettlement {
        settlement_id: String,
        intent_id: String,
        solver_id: String,
        user: String,
        user_input_amount: Uint128,
        user_input_denom: String,
        solver_output_amount: Uint128,
        solver_output_denom: String,
        expires_at: u64,
    },

    /// Mark user funds as locked (called by escrow)
    MarkUserLocked {
        settlement_id: String,
        escrow_id: String,
    },

    /// Mark solver funds as locked
    MarkSolverLocked { settlement_id: String },

    /// Mark settlement as executing
    MarkExecuting { settlement_id: String },

    /// Mark settlement as completed
    MarkCompleted { settlement_id: String },

    /// Mark settlement as failed
    MarkFailed {
        settlement_id: String,
        reason: String,
    },

    /// Slash solver for failed settlement
    SlashSolver {
        solver_id: String,
        settlement_id: String,
    },

    /// Update config (admin only)
    UpdateConfig {
        admin: Option<String>,
        escrow_contract: Option<String>,
        min_solver_bond: Option<Uint128>,
        base_slash_bps: Option<u64>,
    },

    /// Execute settlement via IBC transfer (cross-chain)
    ExecuteSettlement {
        settlement_id: String,
        ibc_channel: String,
    },

    /// Execute settlement via direct bank transfer (same-chain)
    /// This is an atomic operation that:
    /// 1. Transfers solver output to user (via BankMsg::Send)
    /// 2. Releases user's escrow to solver
    /// 3. Marks settlement as completed
    /// Caller must send the solver_output_amount with this message.
    ExecuteSettlementLocal { settlement_id: String },

    /// Handle IBC timeout - refund user and potentially slash solver
    HandleTimeout { settlement_id: String },

    /// Handle IBC acknowledgement
    HandleIbcAck {
        settlement_id: String,
        success: bool,
    },

    /// Update reputation for a solver
    UpdateReputation { solver_id: String },

    /// Decay reputation scores (called periodically)
    DecayReputation {
        start_after: Option<String>,
        limit: Option<u32>,
    },

    // ═══════════════════════════════════════════════════════════════════════════
    // COLLATERAL MANAGEMENT
    // ═══════════════════════════════════════════════════════════════════════════

    /// Deposit collateral to solver's bond pool
    /// Caller must send the collateral tokens with this message
    DepositCollateral {
        solver_id: String,
        /// The type of asset being deposited (for validation)
        asset: CollateralAsset,
    },

    /// Withdraw unlocked collateral from solver's bond pool
    WithdrawCollateral {
        solver_id: String,
        /// The asset to withdraw
        asset: CollateralAsset,
        /// Amount to withdraw
        amount: Uint128,
    },

    /// Update cached pool info for cross-chain Hydro vaults (via ICQ callback)
    UpdatePoolInfo {
        share_denom: String,
        total_shares_issued: Uint128,
        total_pool_value: Uint128,
    },

    /// Submit a bid on a liquidation intent
    BidOnLiquidation {
        liquidation_id: String,
        /// ATOM amount the solver will deliver (must meet min_output_amount)
        offered_output: Uint128,
    },

    /// Execute a liquidation (settle the winning bid)
    /// Caller must send the offered ATOM amount with this message
    ExecuteLiquidation { liquidation_id: String },

    /// Cancel an expired liquidation intent (admin only)
    CancelLiquidation {
        liquidation_id: String,
        reason: String,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(SolverResponse)]
    Solver { solver_id: String },

    #[returns(SettlementResponse)]
    Settlement { settlement_id: String },

    #[returns(SettlementResponse)]
    SettlementByIntent { intent_id: String },

    #[returns(SolversResponse)]
    Solvers {
        start_after: Option<String>,
        limit: Option<u32>,
    },

    #[returns(SettlementsResponse)]
    SettlementsBySolver {
        solver_id: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },

    #[returns(SolverReputationResponse)]
    SolverReputation { solver_id: String },

    #[returns(TopSolversResponse)]
    TopSolvers { limit: u32 },

    #[returns(SolversByReputationResponse)]
    SolversByReputation { min_score: u64, limit: u32 },

    /// Query migration info
    #[returns(MigrationInfoResponse)]
    MigrationInfo {},

    /// Query inflight (non-terminal) settlements
    #[returns(InflightSettlementsResponse)]
    InflightSettlements {
        start_after: Option<String>,
        limit: Option<u32>,
    },

    // ═══════════════════════════════════════════════════════════════════════════
    // COLLATERAL QUERIES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Query a solver's bond pool
    #[returns(BondPoolResponse)]
    BondPool { solver_id: String },

    /// Query total collateral value for a solver (after haircuts)
    #[returns(CollateralValueResponse)]
    CollateralValue { solver_id: String },

    /// Query available (unlocked) collateral for a solver
    #[returns(AvailableCollateralResponse)]
    AvailableCollateral { solver_id: String },

    /// Query a liquidation intent
    #[returns(LiquidationResponse)]
    Liquidation { liquidation_id: String },

    /// Query pending liquidations
    #[returns(LiquidationsResponse)]
    PendingLiquidations {
        start_after: Option<String>,
        limit: Option<u32>,
    },

    /// Query cached pool info for a Hydro vault
    #[returns(PoolInfoResponse)]
    CachedPoolInfo { share_denom: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub admin: String,
    pub escrow_contract: String,
    pub min_solver_bond: Uint128,
    pub base_slash_bps: u64,
}

#[cw_serde]
pub struct SolverResponse {
    pub id: String,
    pub operator: String,
    pub bond_amount: Uint128,
    pub active: bool,
    pub total_settlements: u64,
    pub failed_settlements: u64,
    pub registered_at: u64,
}

#[cw_serde]
pub struct SolversResponse {
    pub solvers: Vec<SolverResponse>,
}

#[cw_serde]
pub struct SettlementResponse {
    pub id: String,
    pub intent_id: String,
    pub solver_id: String,
    pub user: String,
    pub user_input_amount: Uint128,
    pub user_input_denom: String,
    pub solver_output_amount: Uint128,
    pub solver_output_denom: String,
    pub status: String,
    pub created_at: u64,
    pub expires_at: u64,
}

#[cw_serde]
pub struct SettlementsResponse {
    pub settlements: Vec<SettlementResponse>,
}

#[cw_serde]
pub struct SolverReputationResponse {
    pub solver_id: String,
    pub total_settlements: u64,
    pub successful_settlements: u64,
    pub failed_settlements: u64,
    pub total_volume: Uint128,
    pub average_settlement_time: u64,
    pub slashing_events: u64,
    pub reputation_score: u64,
    pub fee_tier: String,
    pub last_updated: u64,
}

#[cw_serde]
pub struct TopSolversResponse {
    pub solvers: Vec<SolverReputationResponse>,
}

#[cw_serde]
pub struct SolversByReputationResponse {
    pub solvers: Vec<SolverReputationResponse>,
}

// ═══════════════════════════════════════════════════════════════════════════
// MIGRATION MESSAGES - For zero-downtime upgrades
// ═══════════════════════════════════════════════════════════════════════════

/// Message for contract migration
#[cw_serde]
pub struct MigrateMsg {
    /// New protocol version
    pub new_version: String,

    /// Migration configuration
    pub config: Option<MigrationConfig>,
}

/// Configuration for how to handle migration
#[cw_serde]
pub struct MigrationConfig {
    /// Preserve inflight settlements during migration (default: true)
    pub preserve_inflight: bool,

    /// Action for stuck settlements that have exceeded timeout
    pub stuck_settlement_action: StuckSettlementAction,

    /// New configuration values (optional)
    pub new_config: Option<ConfigUpdate>,

    /// Extend inflight settlement timeouts by this many seconds
    pub extend_timeout_secs: Option<u64>,
}

/// How to handle settlements that are stuck (past timeout)
#[cw_serde]
pub enum StuckSettlementAction {
    /// Keep as-is, process after migration
    Preserve,

    /// Refund users, mark as failed
    RefundAndFail,

    /// Extend timeout to allow completion
    ExtendTimeout { additional_seconds: u64 },
}

/// Configuration updates to apply during migration
#[cw_serde]
pub struct ConfigUpdate {
    pub admin: Option<String>,
    pub escrow_contract: Option<String>,
    pub min_solver_bond: Option<Uint128>,
    pub base_slash_bps: Option<u64>,
}

/// Response from migration info query
#[cw_serde]
pub struct MigrationInfoResponse {
    /// Contract version before migration
    pub previous_version: Option<String>,
    /// Current contract version
    pub current_version: String,
    /// When migration occurred
    pub migrated_at: Option<u64>,
    /// Number of inflight settlements preserved
    pub preserved_inflight_count: u64,
}

/// Response for inflight settlements query
#[cw_serde]
pub struct InflightSettlementsResponse {
    /// List of settlement IDs that are not in terminal state
    pub settlement_ids: Vec<String>,
    /// Total count
    pub count: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL RESPONSE TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Single collateral deposit info
#[cw_serde]
pub struct CollateralDepositInfo {
    pub asset: CollateralAsset,
    pub asset_class: AssetClass,
    pub total_amount: Uint128,
    pub locked_amount: Uint128,
    pub available_amount: Uint128,
    pub haircut_bps: u64,
}

/// Response for bond pool query
#[cw_serde]
pub struct BondPoolResponse {
    pub solver_id: String,
    pub deposits: Vec<CollateralDepositInfo>,
}

/// Response for total collateral value query
#[cw_serde]
pub struct CollateralValueResponse {
    pub solver_id: String,
    /// Total value after haircuts (in uatom equivalent)
    pub total_value: Uint128,
    /// Breakdown by asset class
    pub by_class: Vec<AssetClassValue>,
}

/// Value by asset class
#[cw_serde]
pub struct AssetClassValue {
    pub class: AssetClass,
    pub raw_value: Uint128,
    pub haircut_bps: u64,
    pub value_after_haircut: Uint128,
}

/// Response for available collateral query
#[cw_serde]
pub struct AvailableCollateralResponse {
    pub solver_id: String,
    /// Total available value after haircuts
    pub available_value: Uint128,
    /// Can the solver take on more settlements?
    pub can_accept_settlements: bool,
}

/// Response for liquidation query
#[cw_serde]
pub struct LiquidationResponse {
    pub id: String,
    pub source_settlement_id: String,
    pub slashed_solver: String,
    pub collateral_asset: CollateralAsset,
    pub collateral_amount: Uint128,
    pub min_output_amount: Uint128,
    pub beneficiary: String,
    pub timeout: u64,
    pub status: LiquidationStatus,
    pub winning_bid: Option<BidInfo>,
}

/// Bid information
#[cw_serde]
pub struct BidInfo {
    pub solver: String,
    pub offered_output: Uint128,
    pub submitted_at: u64,
}

/// Response for pending liquidations query
#[cw_serde]
pub struct LiquidationsResponse {
    pub liquidations: Vec<LiquidationResponse>,
}

/// Response for cached pool info query
#[cw_serde]
pub struct PoolInfoResponse {
    pub share_denom: String,
    pub total_shares_issued: Uint128,
    pub total_pool_value: Uint128,
    pub updated_at: u64,
    pub is_stale: bool,
}
