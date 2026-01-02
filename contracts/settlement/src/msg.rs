use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

use crate::state::{LsmBondConfig, LstBondConfig, LstTokenConfig};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub escrow_contract: String,
    pub min_solver_bond: Uint128,
    pub base_slash_bps: u64,
    /// Optional LSM bond configuration (defaults to enabled with 95% valuation)
    pub lsm_config: Option<LsmBondConfig>,
    /// Optional LST bond configuration (defaults to enabled with standard tokens)
    pub lst_config: Option<LstBondConfig>,
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

    // ═══════════════════════════════════════════════════════════════════════
    // LSM & LST BOND MANAGEMENT
    // ═══════════════════════════════════════════════════════════════════════

    /// Add additional bond assets to an existing solver registration
    /// Accepts native ATOM, LSM shares, and LST tokens
    AddBond { solver_id: String },

    /// Withdraw bond assets from a solver (must maintain minimum bond)
    /// Specifies which assets and amounts to withdraw
    WithdrawBond {
        solver_id: String,
        /// List of (denom, amount) pairs to withdraw
        withdrawals: Vec<(String, Uint128)>,
    },

    /// Update LSM bond configuration (admin only)
    UpdateLsmConfig {
        enabled: Option<bool>,
        blocked_validators: Option<Vec<String>>,
        max_lsm_per_solver: Option<Uint128>,
        valuation_discount_bps: Option<u64>,
    },

    /// Update LST bond configuration (admin only)
    UpdateLstConfig {
        enabled: Option<bool>,
        max_lst_per_solver: Option<Uint128>,
    },

    /// Add or update an accepted LST token (admin only)
    AddOrUpdateLstToken {
        denom: String,
        protocol: String,
        exchange_rate_bps: u64,
        max_per_solver: Option<Uint128>,
        enabled: bool,
    },

    /// Remove an LST token from accepted list (admin only)
    RemoveLstToken { denom: String },

    /// Block a validator for LSM bonding (admin only)
    BlockValidator { validator: String },

    /// Unblock a validator for LSM bonding (admin only)
    UnblockValidator { validator: String },
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

    // ═══════════════════════════════════════════════════════════════════════
    // LSM & LST BOND QUERIES
    // ═══════════════════════════════════════════════════════════════════════

    /// Query detailed bond information for a solver
    #[returns(SolverBondResponse)]
    SolverBond { solver_id: String },

    /// Query LSM bond configuration
    #[returns(LsmConfigResponse)]
    LsmConfig {},

    /// Query LST bond configuration
    #[returns(LstConfigResponse)]
    LstConfig {},

    /// Query accepted LST tokens
    #[returns(AcceptedLstTokensResponse)]
    AcceptedLstTokens {},
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
// LSM & LST BOND RESPONSE TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Detailed bond asset information
#[cw_serde]
pub struct BondAssetResponse {
    /// The denomination of the asset
    pub denom: String,
    /// The raw amount in base units
    pub amount: Uint128,
    /// Type: "native_atom", "lsm_share", or "lst"
    pub asset_type: String,
    /// Additional info (validator for LSM, protocol for LST)
    pub asset_info: Option<String>,
    /// ATOM-equivalent value
    pub atom_value: Uint128,
}

/// Response for solver bond query
#[cw_serde]
pub struct SolverBondResponse {
    pub solver_id: String,
    /// List of all bond assets
    pub assets: Vec<BondAssetResponse>,
    /// Total ATOM-equivalent value
    pub total_atom_value: Uint128,
    /// Native ATOM amount
    pub native_atom_amount: Uint128,
    /// Total LSM share value (in ATOM-equivalent)
    pub lsm_value: Uint128,
    /// Total LST value (in ATOM-equivalent)
    pub lst_value: Uint128,
    /// Last update timestamp
    pub last_updated: u64,
}

/// Response for LSM configuration query
#[cw_serde]
pub struct LsmConfigResponse {
    pub enabled: bool,
    pub blocked_validators: Vec<String>,
    pub max_lsm_per_solver: Option<Uint128>,
    /// Valuation discount in basis points (e.g., 9500 = 95%)
    pub valuation_discount_bps: u64,
}

/// Response for LST configuration query
#[cw_serde]
pub struct LstConfigResponse {
    pub enabled: bool,
    pub max_lst_per_solver: Option<Uint128>,
    pub accepted_tokens_count: u32,
}

/// Response for accepted LST tokens query
#[cw_serde]
pub struct AcceptedLstTokensResponse {
    pub tokens: Vec<LstTokenConfig>,
}
