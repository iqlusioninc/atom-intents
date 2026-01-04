//! Bond asset types for solver bonding with LSM shares and LSTs
//!
//! This module defines the types for multi-asset solver bonding, supporting:
//! - Native ATOM tokens
//! - LSM (Liquid Staking Module) shares from Cosmos Hub
//! - LST (Liquid Staking Tokens) like stATOM, qATOM, stkATOM

use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

/// Represents the type of bond asset
#[cw_serde]
pub enum BondAssetType {
    /// Native ATOM tokens (uatom)
    NativeAtom,

    /// LSM share from Cosmos Hub's Liquid Staking Module
    /// These are tokenized representations of delegated ATOM
    /// Format: cosmosvaloperXXX/YYY where XXX is validator address and YYY is record ID
    LsmShare {
        /// The validator address this LSM share is associated with
        validator: String,
    },

    /// Liquid Staking Token (e.g., stATOM from Stride, qATOM from Quicksilver)
    Lst {
        /// The LST protocol/provider (e.g., "stride", "quicksilver", "pstake")
        protocol: String,
    },
}

impl BondAssetType {
    /// Returns true if this is a native ATOM bond
    pub fn is_native(&self) -> bool {
        matches!(self, BondAssetType::NativeAtom)
    }

    /// Returns true if this is an LSM share
    pub fn is_lsm_share(&self) -> bool {
        matches!(self, BondAssetType::LsmShare { .. })
    }

    /// Returns true if this is an LST
    pub fn is_lst(&self) -> bool {
        matches!(self, BondAssetType::Lst { .. })
    }

    /// Get a human-readable type name
    pub fn type_name(&self) -> &'static str {
        match self {
            BondAssetType::NativeAtom => "native_atom",
            BondAssetType::LsmShare { .. } => "lsm_share",
            BondAssetType::Lst { .. } => "lst",
        }
    }
}

/// A single bond asset held by a solver
#[cw_serde]
pub struct BondAsset {
    /// The denomination of the asset
    pub denom: String,

    /// The raw amount in base units (e.g., uatom, ustATOM)
    pub amount: Uint128,

    /// The type of bond asset
    pub asset_type: BondAssetType,
}

impl BondAsset {
    /// Create a new native ATOM bond
    pub fn native_atom(amount: Uint128) -> Self {
        Self {
            denom: "uatom".to_string(),
            amount,
            asset_type: BondAssetType::NativeAtom,
        }
    }

    /// Create a new LSM share bond
    pub fn lsm_share(denom: String, amount: Uint128, validator: String) -> Self {
        Self {
            denom,
            amount,
            asset_type: BondAssetType::LsmShare { validator },
        }
    }

    /// Create a new LST bond
    pub fn lst(denom: String, amount: Uint128, protocol: String) -> Self {
        Self {
            denom,
            amount,
            asset_type: BondAssetType::Lst { protocol },
        }
    }
}

/// Configuration for accepted LST tokens
#[cw_serde]
pub struct LstConfig {
    /// The denomination (e.g., "stATOM", "qATOM")
    pub denom: String,

    /// The protocol name (e.g., "stride", "quicksilver")
    pub protocol: String,

    /// Exchange rate to ATOM (e.g., 1.05 means 1 LST = 1.05 ATOM)
    /// Stored as basis points where 10000 = 1.0, so 10500 = 1.05
    pub exchange_rate_bps: u64,

    /// Maximum amount that can be bonded (to limit exposure)
    pub max_bond_amount: Option<Uint128>,

    /// Whether this LST is currently accepted for bonding
    pub enabled: bool,
}

impl LstConfig {
    /// Calculate the ATOM-equivalent value for a given LST amount
    pub fn to_atom_value(&self, lst_amount: Uint128) -> Uint128 {
        lst_amount * Uint128::from(self.exchange_rate_bps) / Uint128::from(10000u64)
    }
}

/// Configuration for LSM share acceptance
#[cw_serde]
pub struct LsmConfig {
    /// Whether LSM shares are accepted for bonding
    pub enabled: bool,

    /// Minimum validator commission rate to accept (in basis points)
    /// This prevents accepting shares from validators with 0% commission
    pub min_validator_commission_bps: Option<u64>,

    /// List of blocked validators (e.g., jailed, tombstoned)
    pub blocked_validators: Vec<String>,

    /// Maximum total LSM share value per solver (in uatom equivalent)
    pub max_lsm_bond_per_solver: Option<Uint128>,

    /// Discount factor for LSM shares vs native ATOM (in basis points)
    /// e.g., 9500 means LSM shares are valued at 95% of native ATOM
    /// This accounts for liquidity and redemption risks
    pub valuation_discount_bps: u64,
}

impl Default for LsmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_validator_commission_bps: Some(100), // 1% minimum commission
            blocked_validators: vec![],
            max_lsm_bond_per_solver: None,
            // 5% discount - LSM shares valued at 95% of native ATOM
            valuation_discount_bps: 9500,
        }
    }
}

impl LsmConfig {
    /// Calculate the ATOM-equivalent value for a given LSM share amount
    /// LSM shares are 1:1 with staked ATOM but may have a liquidity discount
    pub fn to_atom_value(&self, lsm_amount: Uint128) -> Uint128 {
        lsm_amount * Uint128::from(self.valuation_discount_bps) / Uint128::from(10000u64)
    }
}

/// Summary of a solver's total bond across all asset types
#[cw_serde]
pub struct BondSummary {
    /// Total value in ATOM-equivalent terms
    pub total_atom_value: Uint128,

    /// Native ATOM amount
    pub native_atom_amount: Uint128,

    /// Total LSM share value (in ATOM-equivalent)
    pub lsm_share_value: Uint128,

    /// Total LST value (in ATOM-equivalent)
    pub lst_value: Uint128,

    /// Number of different assets in the bond
    pub asset_count: u32,
}

/// Known LST protocols and their denominations
pub mod known_lsts {
    /// Stride stATOM
    pub const STATOM_DENOM: &str = "stuatom";
    pub const STATOM_PROTOCOL: &str = "stride";

    /// Quicksilver qATOM
    pub const QATOM_DENOM: &str = "uqatom";
    pub const QATOM_PROTOCOL: &str = "quicksilver";

    /// pSTAKE stkATOM
    pub const STKATOM_DENOM: &str = "stk/uatom";
    pub const STKATOM_PROTOCOL: &str = "pstake";

    /// Check if a denom is a known LST
    pub fn is_known_lst(denom: &str) -> bool {
        matches!(
            denom,
            STATOM_DENOM | QATOM_DENOM | STKATOM_DENOM
        )
    }

    /// Get protocol name for a known LST denom
    pub fn get_protocol(denom: &str) -> Option<&'static str> {
        match denom {
            STATOM_DENOM => Some(STATOM_PROTOCOL),
            QATOM_DENOM => Some(QATOM_PROTOCOL),
            STKATOM_DENOM => Some(STKATOM_PROTOCOL),
            _ => None,
        }
    }
}

/// LSM share denom utilities
pub mod lsm_utils {
    /// Check if a denom looks like an LSM share
    /// LSM share denoms have the format: cosmosvaloperXXX/YYY
    pub fn is_lsm_share_denom(denom: &str) -> bool {
        denom.starts_with("cosmosvaloper") && denom.contains('/')
    }

    /// Extract validator address from LSM share denom
    pub fn extract_validator(denom: &str) -> Option<&str> {
        if !is_lsm_share_denom(denom) {
            return None;
        }
        denom.split('/').next()
    }

    /// Extract record ID from LSM share denom
    pub fn extract_record_id(denom: &str) -> Option<&str> {
        if !is_lsm_share_denom(denom) {
            return None;
        }
        denom.split('/').nth(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bond_asset_type_classification() {
        let native = BondAssetType::NativeAtom;
        assert!(native.is_native());
        assert!(!native.is_lsm_share());
        assert!(!native.is_lst());

        let lsm = BondAssetType::LsmShare {
            validator: "cosmosvaloper1xxx".to_string(),
        };
        assert!(!lsm.is_native());
        assert!(lsm.is_lsm_share());
        assert!(!lsm.is_lst());

        let lst = BondAssetType::Lst {
            protocol: "stride".to_string(),
        };
        assert!(!lst.is_native());
        assert!(!lst.is_lsm_share());
        assert!(lst.is_lst());
    }

    #[test]
    fn test_bond_asset_constructors() {
        let native = BondAsset::native_atom(Uint128::new(1000));
        assert_eq!(native.denom, "uatom");
        assert_eq!(native.amount, Uint128::new(1000));
        assert!(native.asset_type.is_native());

        let lsm = BondAsset::lsm_share(
            "cosmosvaloper1xxx/1".to_string(),
            Uint128::new(500),
            "cosmosvaloper1xxx".to_string(),
        );
        assert!(lsm.asset_type.is_lsm_share());

        let lst = BondAsset::lst(
            "stuatom".to_string(),
            Uint128::new(800),
            "stride".to_string(),
        );
        assert!(lst.asset_type.is_lst());
    }

    #[test]
    fn test_lst_config_valuation() {
        let config = LstConfig {
            denom: "stuatom".to_string(),
            protocol: "stride".to_string(),
            exchange_rate_bps: 10500, // 1.05
            max_bond_amount: None,
            enabled: true,
        };

        // 1000 stATOM at 1.05 rate = 1050 ATOM
        let value = config.to_atom_value(Uint128::new(1000));
        assert_eq!(value, Uint128::new(1050));
    }

    #[test]
    fn test_lsm_config_valuation() {
        let config = LsmConfig {
            enabled: true,
            min_validator_commission_bps: Some(100),
            blocked_validators: vec![],
            max_lsm_bond_per_solver: None,
            valuation_discount_bps: 9500, // 95%
        };

        // 1000 LSM shares at 95% = 950 ATOM equivalent
        let value = config.to_atom_value(Uint128::new(1000));
        assert_eq!(value, Uint128::new(950));
    }

    #[test]
    fn test_lsm_share_denom_utils() {
        use lsm_utils::*;

        // Valid LSM share denom
        assert!(is_lsm_share_denom("cosmosvaloper1xyz/1"));
        assert_eq!(extract_validator("cosmosvaloper1xyz/1"), Some("cosmosvaloper1xyz"));
        assert_eq!(extract_record_id("cosmosvaloper1xyz/1"), Some("1"));

        // Invalid denoms
        assert!(!is_lsm_share_denom("uatom"));
        assert!(!is_lsm_share_denom("stuatom"));
        assert!(!is_lsm_share_denom("cosmosvaloper1xyz")); // No record ID
    }

    #[test]
    fn test_known_lsts() {
        use known_lsts::*;

        assert!(is_known_lst(STATOM_DENOM));
        assert!(is_known_lst(QATOM_DENOM));
        assert!(is_known_lst(STKATOM_DENOM));
        assert!(!is_known_lst("uatom"));

        assert_eq!(get_protocol(STATOM_DENOM), Some("stride"));
        assert_eq!(get_protocol(QATOM_DENOM), Some("quicksilver"));
        assert_eq!(get_protocol("unknown"), None);
    }
}
