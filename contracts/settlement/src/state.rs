use atom_intents_types::collateral::{LiquidationIntent, LockedCollateral, SolverBondPool};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    /// Admin address
    pub admin: Addr,
    /// Escrow contract address
    pub escrow_contract: Addr,
    /// Minimum solver bond amount
    pub min_solver_bond: Uint128,
    /// Base slash percentage (e.g., 200 = 2%)
    pub base_slash_bps: u64,
}

#[cw_serde]
pub struct RegisteredSolver {
    pub id: String,
    pub operator: Addr,
    pub bond_amount: Uint128,
    pub active: bool,
    pub total_settlements: u64,
    pub failed_settlements: u64,
    pub registered_at: u64,
}

#[cw_serde]
pub struct Settlement {
    pub id: String,
    pub intent_id: String,
    pub solver_id: String,
    pub user: Addr,
    pub user_input_amount: Uint128,
    pub user_input_denom: String,
    pub solver_output_amount: Uint128,
    pub solver_output_denom: String,
    pub status: SettlementStatus,
    pub created_at: u64,
    pub expires_at: u64,
    pub escrow_id: Option<String>,
    /// Collateral locked for this settlement (1.5x fill value)
    pub locked_collateral: Option<LockedCollateral>,
}

/// Minimum slash amount to prevent dust attacks (10 ATOM = 10_000_000 uatom)
pub const MIN_SLASH_AMOUNT: u128 = 10_000_000;

#[cw_serde]
pub enum SettlementStatus {
    Pending,
    UserLocked,
    SolverLocked,
    Executing,
    Completed,
    Failed { reason: String },
    Slashed { amount: Uint128 },
}

impl SettlementStatus {
    /// SECURITY FIX (5.6/7.1): Validates state machine transitions
    ///
    /// Valid transitions:
    /// - Pending -> UserLocked | Failed | Slashed
    /// - UserLocked -> SolverLocked | Failed | Slashed
    /// - SolverLocked -> Executing | Failed | Slashed
    /// - Executing -> Completed | Failed | Slashed
    /// - Failed -> Slashed (for penalty after failure)
    ///
    /// Slashing is allowed from any active state because solvers can misbehave
    /// at any point after taking a position on a settlement.
    ///
    /// Invalid transitions return false.
    pub fn can_transition_to(&self, target: &SettlementStatus) -> bool {
        match (self, target) {
            // Normal flow
            (SettlementStatus::Pending, SettlementStatus::UserLocked) => true,
            (SettlementStatus::UserLocked, SettlementStatus::SolverLocked) => true,
            (SettlementStatus::SolverLocked, SettlementStatus::Executing) => true,
            (SettlementStatus::Executing, SettlementStatus::Completed) => true,

            // Failure can happen from any active state
            (SettlementStatus::Pending, SettlementStatus::Failed { .. }) => true,
            (SettlementStatus::UserLocked, SettlementStatus::Failed { .. }) => true,
            (SettlementStatus::SolverLocked, SettlementStatus::Failed { .. }) => true,
            (SettlementStatus::Executing, SettlementStatus::Failed { .. }) => true,

            // Slashing can happen from any active state or after failure
            (SettlementStatus::Pending, SettlementStatus::Slashed { .. }) => true,
            (SettlementStatus::UserLocked, SettlementStatus::Slashed { .. }) => true,
            (SettlementStatus::SolverLocked, SettlementStatus::Slashed { .. }) => true,
            (SettlementStatus::Executing, SettlementStatus::Slashed { .. }) => true,
            (SettlementStatus::Failed { .. }, SettlementStatus::Slashed { .. }) => true,

            // All other transitions are invalid (e.g., Completed -> anything)
            _ => false,
        }
    }

    /// Returns a string representation for error messages
    pub fn as_str(&self) -> &'static str {
        match self {
            SettlementStatus::Pending => "Pending",
            SettlementStatus::UserLocked => "UserLocked",
            SettlementStatus::SolverLocked => "SolverLocked",
            SettlementStatus::Executing => "Executing",
            SettlementStatus::Completed => "Completed",
            SettlementStatus::Failed { .. } => "Failed",
            SettlementStatus::Slashed { .. } => "Slashed",
        }
    }
}

#[cw_serde]
pub struct SolverReputation {
    pub solver_id: String,
    pub total_settlements: u64,
    pub successful_settlements: u64,
    pub failed_settlements: u64,
    pub total_volume: Uint128,
    pub average_settlement_time: u64, // seconds
    pub slashing_events: u64,
    pub reputation_score: u64, // 0-10000 (basis points)
    pub last_updated: u64,
}

#[cw_serde]
pub enum FeeTier {
    Premium,  // 9000-10000 score - lowest fees
    Standard, // 7000-8999 score
    Basic,    // 5000-6999 score
    New,      // 0-4999 score - highest fees (new/low rep solvers)
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const SOLVERS: Map<&str, RegisteredSolver> = Map::new("solvers");
pub const SETTLEMENTS: Map<&str, Settlement> = Map::new("settlements");
pub const INTENT_SETTLEMENTS: Map<&str, String> = Map::new("intent_settlements");
pub const REPUTATIONS: Map<&str, SolverReputation> = Map::new("reputations");

// ═══════════════════════════════════════════════════════════════════════════
// MIGRATION STATE
// ═══════════════════════════════════════════════════════════════════════════

/// Tracks migration history for the contract
#[cw_serde]
pub struct MigrationInfo {
    /// Previous version before last migration
    pub previous_version: Option<String>,
    /// Current version
    pub current_version: String,
    /// Timestamp of last migration
    pub migrated_at: Option<u64>,
    /// Number of inflight settlements preserved during last migration
    pub preserved_inflight_count: u64,
}

pub const MIGRATION_INFO: Item<MigrationInfo> = Item::new("migration_info");

// ═══════════════════════════════════════════════════════════════════════════
// BOND POOL STATE
// ═══════════════════════════════════════════════════════════════════════════

/// Solver bond pools - multi-asset collateral for per-settlement locking
pub const BOND_POOLS: Map<&str, SolverBondPool> = Map::new("bond_pools");

/// Cached pool info for cross-chain Hydro vaults (keyed by share_denom)
pub const CACHED_POOL_INFO: Map<&str, atom_intents_types::collateral::CachedPoolInfo> =
    Map::new("cached_pool_info");

/// Liquidation intents (keyed by intent ID)
pub const LIQUIDATION_INTENTS: Map<&str, LiquidationIntent> = Map::new("liquidation_intents");

/// Index: settlement_id -> liquidation_intent_id (for lookups)
pub const SETTLEMENT_LIQUIDATIONS: Map<&str, String> = Map::new("settlement_liquidations");
