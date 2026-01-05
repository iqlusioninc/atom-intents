use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};

// ═══════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════

/// Fixed haircuts per asset class (basis points)
pub const HAIRCUT_NATIVE_BPS: u64 = 0; // 0%
pub const HAIRCUT_LSM_BPS: u64 = 1000; // 10%
pub const HAIRCUT_HYDRO_BPS: u64 = 2000; // 20%

/// Maximum staleness for cached cross-chain valuations (1 hour)
pub const MAX_STALENESS_SECONDS: u64 = 3600;

/// Lock multiplier for per-settlement bonding (1.5x = 15000 bps)
pub const LOCK_MULTIPLIER_BPS: u64 = 15000;

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL ASSET TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Supported collateral asset types for solver bonds
#[cw_serde]
pub enum CollateralAsset {
    /// Native liquid token (ATOM, USDC)
    Native { denom: String },

    /// Hydro Inflow vault shares
    HydroVault {
        /// Address of the vault contract
        vault_contract: Addr,
        /// The share token denom (factory/neutron1.../inflow_atom)
        share_denom: String,
        /// Chain ID where vault lives (for ICQ routing)
        chain_id: String,
    },

    /// LSM tokenized delegation
    LsmShare {
        /// Validator operator address
        validator: String,
        /// Share token denom (cosmosvaloper1.../42)
        share_denom: String,
    },
}

impl CollateralAsset {
    /// Get the asset class for this collateral
    pub fn asset_class(&self) -> AssetClass {
        match self {
            CollateralAsset::Native { .. } => AssetClass::Native,
            CollateralAsset::HydroVault { .. } => AssetClass::HydroVault,
            CollateralAsset::LsmShare { .. } => AssetClass::LsmShare,
        }
    }

    /// Get the token denom for this collateral (for bank transfers)
    pub fn denom(&self) -> &str {
        match self {
            CollateralAsset::Native { denom } => denom,
            CollateralAsset::HydroVault { share_denom, .. } => share_denom,
            CollateralAsset::LsmShare { share_denom, .. } => share_denom,
        }
    }

    /// Get the haircut basis points for this asset
    pub fn haircut_bps(&self) -> u64 {
        match self.asset_class() {
            AssetClass::Native => HAIRCUT_NATIVE_BPS,
            AssetClass::LsmShare => HAIRCUT_LSM_BPS,
            AssetClass::HydroVault => HAIRCUT_HYDRO_BPS,
        }
    }
}

/// Asset classification for haircut lookup and grouping
#[cw_serde]
#[derive(Copy, Eq, Hash)]
pub enum AssetClass {
    Native,
    LsmShare,
    HydroVault,
}

impl AssetClass {
    /// Get the haircut basis points for this asset class
    pub fn haircut_bps(&self) -> u64 {
        match self {
            AssetClass::Native => HAIRCUT_NATIVE_BPS,
            AssetClass::LsmShare => HAIRCUT_LSM_BPS,
            AssetClass::HydroVault => HAIRCUT_HYDRO_BPS,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CACHED POOL INFO (FOR CROSS-CHAIN HYDRO VAULTS)
// ═══════════════════════════════════════════════════════════════════════════

/// Cached pool info for cross-chain Hydro vaults
#[cw_serde]
pub struct CachedPoolInfo {
    /// Total shares issued by the vault
    pub total_shares_issued: Uint128,
    /// Total pool value in base tokens
    pub total_pool_value: Uint128,
    /// Timestamp when this cache was updated
    pub updated_at: u64,
}

impl CachedPoolInfo {
    /// Check if this cache entry is stale
    pub fn is_stale(&self, current_time: u64) -> bool {
        current_time.saturating_sub(self.updated_at) > MAX_STALENESS_SECONDS
    }

    /// Calculate share value using this pool info
    /// Returns: amount * (total_pool_value / total_shares_issued)
    pub fn calculate_share_value(&self, amount: Uint128) -> Option<Uint128> {
        if self.total_shares_issued.is_zero() {
            // 1:1 if no shares issued yet
            return Some(amount);
        }

        amount
            .checked_multiply_ratio(self.total_pool_value, self.total_shares_issued)
            .ok()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL DEPOSIT & BOND POOL
// ═══════════════════════════════════════════════════════════════════════════

/// A solver's collateral deposit
#[cw_serde]
pub struct CollateralDeposit {
    /// The collateral asset
    pub asset: CollateralAsset,
    /// Total amount deposited
    pub amount: Uint128,
    /// Amount currently locked in active settlements
    pub locked_amount: Uint128,
}

impl CollateralDeposit {
    /// Create a new deposit
    pub fn new(asset: CollateralAsset, amount: Uint128) -> Self {
        Self {
            asset,
            amount,
            locked_amount: Uint128::zero(),
        }
    }

    /// Get available (unlocked) amount
    pub fn available(&self) -> Uint128 {
        self.amount.saturating_sub(self.locked_amount)
    }

    /// Lock an amount for a settlement
    pub fn lock(&mut self, amount: Uint128) -> Result<(), CollateralError> {
        if self.available() < amount {
            return Err(CollateralError::InsufficientAvailable {
                requested: amount,
                available: self.available(),
            });
        }
        self.locked_amount = self.locked_amount.checked_add(amount).unwrap();
        Ok(())
    }

    /// Unlock an amount after settlement completes
    pub fn unlock(&mut self, amount: Uint128) -> Result<(), CollateralError> {
        if self.locked_amount < amount {
            return Err(CollateralError::UnlockExceedsLocked {
                requested: amount,
                locked: self.locked_amount,
            });
        }
        self.locked_amount = self.locked_amount.checked_sub(amount).unwrap();
        Ok(())
    }
}

/// Solver's complete bond pool with multi-asset support
#[cw_serde]
pub struct SolverBondPool {
    /// Solver identifier
    pub solver_id: String,
    /// All collateral deposits
    pub deposits: Vec<CollateralDeposit>,
}

impl SolverBondPool {
    /// Create a new empty bond pool
    pub fn new(solver_id: String) -> Self {
        Self {
            solver_id,
            deposits: Vec::new(),
        }
    }

    /// Add a deposit to the pool
    pub fn add_deposit(&mut self, asset: CollateralAsset, amount: Uint128) {
        // Check if we already have a deposit of this type
        if let Some(deposit) = self.deposits.iter_mut().find(|d| d.asset == asset) {
            deposit.amount = deposit.amount.checked_add(amount).unwrap();
        } else {
            self.deposits.push(CollateralDeposit::new(asset, amount));
        }
    }

    /// Find a deposit by asset class
    pub fn find_by_class(&self, class: AssetClass) -> Option<&CollateralDeposit> {
        self.deposits
            .iter()
            .find(|d| d.asset.asset_class() == class)
    }

    /// Find a mutable deposit by asset class
    pub fn find_by_class_mut(&mut self, class: AssetClass) -> Option<&mut CollateralDeposit> {
        self.deposits
            .iter_mut()
            .find(|d| d.asset.asset_class() == class)
    }

    /// Find a deposit by exact asset
    pub fn find_by_asset(&self, asset: &CollateralAsset) -> Option<&CollateralDeposit> {
        self.deposits.iter().find(|d| &d.asset == asset)
    }

    /// Find a mutable deposit by exact asset
    pub fn find_by_asset_mut(&mut self, asset: &CollateralAsset) -> Option<&mut CollateralDeposit> {
        self.deposits.iter_mut().find(|d| &d.asset == asset)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LOCKED COLLATERAL (PER-SETTLEMENT)
// ═══════════════════════════════════════════════════════════════════════════

/// Record of collateral locked for a specific settlement
#[cw_serde]
pub struct LockedCollateral {
    /// The collateral asset
    pub asset: CollateralAsset,
    /// Raw amount locked
    pub amount: Uint128,
    /// Collateral value at time of locking (after haircut)
    pub value: Uint128,
}

// ═══════════════════════════════════════════════════════════════════════════
// LIQUIDATION INTENT
// ═══════════════════════════════════════════════════════════════════════════

/// Liquidation intent created when slashing non-native collateral
#[cw_serde]
pub struct LiquidationIntent {
    /// Unique identifier
    pub id: String,
    /// Reference to the failed settlement
    pub source_settlement_id: String,
    /// The slashed solver (excluded from bidding)
    pub slashed_solver: String,
    /// Seized collateral to liquidate
    pub collateral: LockedCollateral,
    /// Minimum ATOM output (covers user compensation)
    pub min_output_amount: Uint128,
    /// User to receive liquidation proceeds
    pub beneficiary: Addr,
    /// Deadline for liquidation (unix timestamp)
    pub timeout: u64,
    /// Current status
    pub status: LiquidationStatus,
}

/// Status of a liquidation intent
#[cw_serde]
pub enum LiquidationStatus {
    /// Waiting for bids
    Pending,
    /// Auction in progress
    Auctioning,
    /// Successfully settled
    Settled { output_amount: Uint128 },
    /// Failed to liquidate
    Failed { reason: String },
}

/// A bid on a liquidation intent
#[cw_serde]
pub struct LiquidationBid {
    /// Bidding solver
    pub solver: Addr,
    /// ATOM amount solver will deliver
    pub offered_output: Uint128,
    /// When the bid was submitted
    pub submitted_at: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// ERRORS
// ═══════════════════════════════════════════════════════════════════════════

/// Errors related to collateral operations
#[derive(Debug, Clone, PartialEq)]
pub enum CollateralError {
    /// Insufficient available collateral for the operation
    InsufficientAvailable {
        requested: Uint128,
        available: Uint128,
    },
    /// Attempting to unlock more than is locked
    UnlockExceedsLocked { requested: Uint128, locked: Uint128 },
    /// No deposit found for the specified asset class
    NoDepositForClass { class: AssetClass },
    /// Stale pool info for cross-chain valuation
    StalePoolInfo { age_seconds: u64 },
    /// No cached pool info available
    NoCachedPoolInfo { share_denom: String },
    /// Validator not found for LSM share
    ValidatorNotFound { validator: String },
    /// Unsupported native denom (not uatom)
    UnsupportedNativeDenom { denom: String },
}

impl std::fmt::Display for CollateralError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollateralError::InsufficientAvailable {
                requested,
                available,
            } => {
                write!(
                    f,
                    "Insufficient available collateral: requested {}, available {}",
                    requested, available
                )
            }
            CollateralError::UnlockExceedsLocked { requested, locked } => {
                write!(
                    f,
                    "Cannot unlock {} - only {} is locked",
                    requested, locked
                )
            }
            CollateralError::NoDepositForClass { class } => {
                write!(f, "No deposit found for asset class {:?}", class)
            }
            CollateralError::StalePoolInfo { age_seconds } => {
                write!(
                    f,
                    "Pool info is stale: {} seconds old (max {})",
                    age_seconds, MAX_STALENESS_SECONDS
                )
            }
            CollateralError::NoCachedPoolInfo { share_denom } => {
                write!(f, "No cached pool info for {}", share_denom)
            }
            CollateralError::ValidatorNotFound { validator } => {
                write!(f, "Validator not found: {}", validator)
            }
            CollateralError::UnsupportedNativeDenom { denom } => {
                write!(f, "Unsupported native denom: {}", denom)
            }
        }
    }
}

impl std::error::Error for CollateralError {}

// ═══════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════

/// Apply haircut to a raw value
pub fn apply_haircut(raw_value: Uint128, haircut_bps: u64) -> Uint128 {
    let haircut = raw_value
        .checked_mul(Uint128::from(haircut_bps))
        .unwrap_or(Uint128::zero())
        .checked_div(Uint128::from(10000u64))
        .unwrap_or(Uint128::zero());

    raw_value.saturating_sub(haircut)
}

/// Calculate raw amount needed to achieve a target collateral value after haircut
pub fn reverse_haircut(target_value: Uint128, haircut_bps: u64) -> Uint128 {
    // raw_value * (10000 - haircut_bps) / 10000 = target_value
    // raw_value = target_value * 10000 / (10000 - haircut_bps)
    let effective_bps = 10000u64.saturating_sub(haircut_bps);
    if effective_bps == 0 {
        return Uint128::MAX; // Would need infinite collateral
    }

    target_value
        .checked_mul(Uint128::from(10000u64))
        .unwrap_or(Uint128::MAX)
        .checked_div(Uint128::from(effective_bps))
        .unwrap_or(Uint128::MAX)
}

/// Calculate required lock amount for a settlement (1.5x fill value)
pub fn calculate_lock_requirement(fill_value: Uint128) -> Uint128 {
    fill_value
        .checked_mul(Uint128::from(LOCK_MULTIPLIER_BPS))
        .unwrap_or(Uint128::MAX)
        .checked_div(Uint128::from(10000u64))
        .unwrap_or(Uint128::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_haircut() {
        // 0% haircut
        assert_eq!(apply_haircut(Uint128::new(1000), 0), Uint128::new(1000));

        // 10% haircut
        assert_eq!(apply_haircut(Uint128::new(1000), 1000), Uint128::new(900));

        // 20% haircut
        assert_eq!(apply_haircut(Uint128::new(1000), 2000), Uint128::new(800));
    }

    #[test]
    fn test_reverse_haircut() {
        // 0% haircut - need exact amount
        assert_eq!(reverse_haircut(Uint128::new(1000), 0), Uint128::new(1000));

        // 10% haircut - need ~1111 to get 1000 after haircut
        let raw = reverse_haircut(Uint128::new(1000), 1000);
        assert_eq!(apply_haircut(raw, 1000), Uint128::new(1000));

        // 20% haircut - need 1250 to get 1000 after haircut
        let raw = reverse_haircut(Uint128::new(1000), 2000);
        assert_eq!(apply_haircut(raw, 2000), Uint128::new(1000));
    }

    #[test]
    fn test_calculate_lock_requirement() {
        // 1.5x of 1000 = 1500
        assert_eq!(
            calculate_lock_requirement(Uint128::new(1000)),
            Uint128::new(1500)
        );
    }

    #[test]
    fn test_collateral_deposit_lock_unlock() {
        let mut deposit = CollateralDeposit::new(
            CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
            Uint128::new(1000),
        );

        assert_eq!(deposit.available(), Uint128::new(1000));

        // Lock 400
        deposit.lock(Uint128::new(400)).unwrap();
        assert_eq!(deposit.available(), Uint128::new(600));
        assert_eq!(deposit.locked_amount, Uint128::new(400));

        // Try to lock too much - should fail
        let result = deposit.lock(Uint128::new(700));
        assert!(result.is_err());

        // Unlock 200
        deposit.unlock(Uint128::new(200)).unwrap();
        assert_eq!(deposit.available(), Uint128::new(800));
        assert_eq!(deposit.locked_amount, Uint128::new(200));
    }

    #[test]
    fn test_cached_pool_info_staleness() {
        let info = CachedPoolInfo {
            total_shares_issued: Uint128::new(1000),
            total_pool_value: Uint128::new(1100),
            updated_at: 0,
        };

        // Fresh
        assert!(!info.is_stale(1000));

        // Still fresh at max staleness
        assert!(!info.is_stale(MAX_STALENESS_SECONDS));

        // Stale after max
        assert!(info.is_stale(MAX_STALENESS_SECONDS + 1));
    }

    #[test]
    fn test_cached_pool_info_share_value() {
        let info = CachedPoolInfo {
            total_shares_issued: Uint128::new(1000),
            total_pool_value: Uint128::new(1100), // 10% yield
            updated_at: 0,
        };

        // 100 shares worth 110 base tokens
        let value = info.calculate_share_value(Uint128::new(100)).unwrap();
        assert_eq!(value, Uint128::new(110));
    }

    #[test]
    fn test_asset_class_haircuts() {
        assert_eq!(AssetClass::Native.haircut_bps(), 0);
        assert_eq!(AssetClass::LsmShare.haircut_bps(), 1000);
        assert_eq!(AssetClass::HydroVault.haircut_bps(), 2000);
    }

    #[test]
    fn test_solver_bond_pool() {
        let mut pool = SolverBondPool::new("solver1".to_string());

        // Add native deposit
        pool.add_deposit(
            CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
            Uint128::new(1000),
        );

        assert_eq!(pool.deposits.len(), 1);
        assert!(pool.find_by_class(AssetClass::Native).is_some());
        assert!(pool.find_by_class(AssetClass::LsmShare).is_none());

        // Add more to same asset
        pool.add_deposit(
            CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
            Uint128::new(500),
        );

        assert_eq!(pool.deposits.len(), 1);
        assert_eq!(
            pool.find_by_class(AssetClass::Native).unwrap().amount,
            Uint128::new(1500)
        );

        // Add LSM deposit
        pool.add_deposit(
            CollateralAsset::LsmShare {
                validator: "cosmosvaloper1xxx".to_string(),
                share_denom: "cosmosvaloper1xxx/1".to_string(),
            },
            Uint128::new(2000),
        );

        assert_eq!(pool.deposits.len(), 2);
        assert!(pool.find_by_class(AssetClass::LsmShare).is_some());
    }
}
