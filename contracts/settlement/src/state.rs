use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    /// Admin address
    pub admin: Addr,
    /// Escrow contract address
    pub escrow_contract: Addr,
    /// Minimum solver bond amount (in ATOM-equivalent value)
    pub min_solver_bond: Uint128,
    /// Base slash percentage (e.g., 200 = 2%)
    pub base_slash_bps: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// LSM & LST BOND CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════

/// Configuration for LSM (Liquid Staking Module) share acceptance
#[cw_serde]
pub struct LsmBondConfig {
    /// Whether LSM shares are accepted for bonding
    pub enabled: bool,
    /// List of blocked validators (jailed, tombstoned, etc.)
    pub blocked_validators: Vec<String>,
    /// Maximum total LSM share value per solver (in uatom equivalent)
    pub max_lsm_per_solver: Option<Uint128>,
    /// Discount factor for LSM shares vs native ATOM (in basis points)
    /// e.g., 9500 means LSM shares are valued at 95% of native ATOM
    pub valuation_discount_bps: u64,
}

impl Default for LsmBondConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            blocked_validators: vec![],
            max_lsm_per_solver: None,
            valuation_discount_bps: 9500, // 95% - 5% discount for liquidity risk
        }
    }
}

/// Configuration for a specific LST token acceptance
#[cw_serde]
pub struct LstTokenConfig {
    /// The denomination (e.g., "stuatom", "uqatom")
    pub denom: String,
    /// The protocol name (e.g., "stride", "quicksilver")
    pub protocol: String,
    /// Exchange rate to ATOM in basis points (10000 = 1.0, 10500 = 1.05)
    pub exchange_rate_bps: u64,
    /// Maximum amount that can be bonded per solver
    pub max_per_solver: Option<Uint128>,
    /// Whether this LST is currently accepted
    pub enabled: bool,
}

/// Global LST bonding configuration
#[cw_serde]
pub struct LstBondConfig {
    /// Whether LST bonding is enabled globally
    pub enabled: bool,
    /// Maximum total LST value per solver (in uatom equivalent)
    pub max_lst_per_solver: Option<Uint128>,
    /// Accepted LST tokens with their configurations
    pub accepted_tokens: Vec<LstTokenConfig>,
}

impl Default for LstBondConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_lst_per_solver: None,
            accepted_tokens: vec![
                // Stride stATOM - default ~5% premium
                LstTokenConfig {
                    denom: "stuatom".to_string(),
                    protocol: "stride".to_string(),
                    exchange_rate_bps: 10500, // 1.05 ATOM per stATOM
                    max_per_solver: None,
                    enabled: true,
                },
                // Quicksilver qATOM - default ~3% premium
                LstTokenConfig {
                    denom: "uqatom".to_string(),
                    protocol: "quicksilver".to_string(),
                    exchange_rate_bps: 10300, // 1.03 ATOM per qATOM
                    max_per_solver: None,
                    enabled: true,
                },
                // pSTAKE stkATOM - default ~3% premium
                LstTokenConfig {
                    denom: "stk/uatom".to_string(),
                    protocol: "pstake".to_string(),
                    exchange_rate_bps: 10300, // 1.03 ATOM per stkATOM
                    max_per_solver: None,
                    enabled: true,
                },
            ],
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// BOND ASSET TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Represents the type of bond asset
#[cw_serde]
pub enum BondAssetType {
    /// Native ATOM tokens (uatom)
    NativeAtom,
    /// LSM share from Cosmos Hub's Liquid Staking Module
    LsmShare { validator: String },
    /// Liquid Staking Token (e.g., stATOM, qATOM)
    Lst { protocol: String },
}

/// A single bond asset held by a solver
#[cw_serde]
pub struct BondAsset {
    /// The denomination of the asset
    pub denom: String,
    /// The raw amount in base units
    pub amount: Uint128,
    /// The type of bond asset
    pub asset_type: BondAssetType,
    /// ATOM-equivalent value at time of deposit
    pub atom_value: Uint128,
}

/// Solver's complete bond holdings
#[cw_serde]
pub struct SolverBond {
    /// List of all bond assets
    pub assets: Vec<BondAsset>,
    /// Total ATOM-equivalent value
    pub total_atom_value: Uint128,
    /// Last update timestamp
    pub last_updated: u64,
}

impl Default for SolverBond {
    fn default() -> Self {
        Self {
            assets: vec![],
            total_atom_value: Uint128::zero(),
            last_updated: 0,
        }
    }
}

impl SolverBond {
    /// Recalculate total ATOM value from all assets
    pub fn recalculate_total(&mut self) {
        self.total_atom_value = self.assets.iter().map(|a| a.atom_value).sum();
    }

    /// Add a bond asset
    pub fn add_asset(&mut self, asset: BondAsset) {
        // Check if we already have this denom
        if let Some(existing) = self.assets.iter_mut().find(|a| a.denom == asset.denom) {
            existing.amount += asset.amount;
            existing.atom_value += asset.atom_value;
        } else {
            self.assets.push(asset);
        }
        self.recalculate_total();
    }

    /// Remove/reduce a bond asset, returns the amount actually removed
    pub fn remove_asset(&mut self, denom: &str, amount: Uint128) -> Option<(Uint128, Uint128)> {
        if let Some(idx) = self.assets.iter().position(|a| a.denom == denom) {
            let asset = &mut self.assets[idx];
            let remove_amount = std::cmp::min(amount, asset.amount);

            // Calculate proportional ATOM value being removed
            let remove_value = if asset.amount.is_zero() {
                Uint128::zero()
            } else {
                asset.atom_value * remove_amount / asset.amount
            };

            asset.amount = asset.amount.saturating_sub(remove_amount);
            asset.atom_value = asset.atom_value.saturating_sub(remove_value);

            // Remove asset if fully depleted
            if asset.amount.is_zero() {
                self.assets.remove(idx);
            }

            self.recalculate_total();
            Some((remove_amount, remove_value))
        } else {
            None
        }
    }

    /// Get native ATOM amount
    pub fn native_atom_amount(&self) -> Uint128 {
        self.assets
            .iter()
            .filter(|a| a.denom == "uatom")
            .map(|a| a.amount)
            .sum()
    }

    /// Get all assets as Coins for returning to solver
    pub fn to_coins(&self) -> Vec<Coin> {
        self.assets
            .iter()
            .filter(|a| !a.amount.is_zero())
            .map(|a| Coin {
                denom: a.denom.clone(),
                amount: a.amount,
            })
            .collect()
    }
}

#[cw_serde]
pub struct RegisteredSolver {
    pub id: String,
    pub operator: Addr,
    /// Legacy field for backward compatibility - total ATOM-equivalent bond value
    pub bond_amount: Uint128,
    /// Detailed bond holdings (LSM shares, LSTs, native ATOM)
    pub bond: SolverBond,
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
// LSM & LST BOND CONFIGURATION STORAGE
// ═══════════════════════════════════════════════════════════════════════════

pub const LSM_CONFIG: Item<LsmBondConfig> = Item::new("lsm_config");
pub const LST_CONFIG: Item<LstBondConfig> = Item::new("lst_config");

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
// LSM SHARE UTILITIES
// ═══════════════════════════════════════════════════════════════════════════

/// Check if a denom is an LSM share denom
/// LSM share denoms have the format: cosmosvaloperXXX/YYY
pub fn is_lsm_share_denom(denom: &str) -> bool {
    denom.starts_with("cosmosvaloper") && denom.contains('/')
}

/// Extract validator address from LSM share denom
pub fn extract_validator_from_lsm(denom: &str) -> Option<String> {
    if !is_lsm_share_denom(denom) {
        return None;
    }
    denom.split('/').next().map(|s| s.to_string())
}
