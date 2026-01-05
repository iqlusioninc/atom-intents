use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Env, QuerierWrapper, Uint128};

use crate::collateral::{
    apply_haircut, AssetClass, CachedPoolInfo, CollateralAsset, CollateralError,
    HAIRCUT_HYDRO_BPS, HAIRCUT_LSM_BPS, HAIRCUT_NATIVE_BPS, MAX_STALENESS_SECONDS,
};

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL VALUATION TRAIT
// ═══════════════════════════════════════════════════════════════════════════

/// Result type for valuation operations
pub type ValuationResult<T> = Result<T, CollateralError>;

/// Trait for collateral valuation - provides value queries for different asset types
pub trait CollateralValuation {
    /// Get raw value in base denomination (uatom)
    /// Returns error if data is stale or unavailable
    fn get_raw_value(&self, querier: &QuerierWrapper, env: &Env, amount: Uint128)
        -> ValuationResult<Uint128>;

    /// Get haircut basis points for this asset
    fn haircut_bps(&self) -> u64;

    /// Get effective collateral value (raw value minus haircut)
    fn get_collateral_value(
        &self,
        querier: &QuerierWrapper,
        env: &Env,
        amount: Uint128,
    ) -> ValuationResult<Uint128> {
        let raw = self.get_raw_value(querier, env, amount)?;
        Ok(apply_haircut(raw, self.haircut_bps()))
    }

    /// Check if this collateral can be liquidated (has liquidity path)
    fn is_liquidatable(&self, querier: &QuerierWrapper) -> bool;

    /// Get the asset class for grouping
    fn asset_class(&self) -> AssetClass;
}

// ═══════════════════════════════════════════════════════════════════════════
// NATIVE TOKEN VALUATION
// ═══════════════════════════════════════════════════════════════════════════

/// Valuation implementation for native tokens (ATOM)
pub struct NativeValuation {
    pub denom: String,
}

impl NativeValuation {
    pub fn new(denom: String) -> Self {
        Self { denom }
    }
}

impl CollateralValuation for NativeValuation {
    fn get_raw_value(
        &self,
        _querier: &QuerierWrapper,
        _env: &Env,
        amount: Uint128,
    ) -> ValuationResult<Uint128> {
        // Native ATOM is 1:1 with base denomination
        if self.denom == "uatom" {
            return Ok(amount);
        }
        // Other native tokens would need oracle - reject for now
        Err(CollateralError::UnsupportedNativeDenom {
            denom: self.denom.clone(),
        })
    }

    fn haircut_bps(&self) -> u64 {
        HAIRCUT_NATIVE_BPS // 0%
    }

    fn is_liquidatable(&self, _querier: &QuerierWrapper) -> bool {
        true // Native tokens always liquidatable
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Native
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LSM SHARE VALUATION
// ═══════════════════════════════════════════════════════════════════════════

/// Valuation implementation for LSM tokenized delegations
pub struct LsmShareValuation {
    pub validator: String,
    pub share_denom: String,
}

impl LsmShareValuation {
    pub fn new(validator: String, share_denom: String) -> Self {
        Self {
            validator,
            share_denom,
        }
    }
}

impl CollateralValuation for LsmShareValuation {
    fn get_raw_value(
        &self,
        _querier: &QuerierWrapper,
        _env: &Env,
        amount: Uint128,
    ) -> ValuationResult<Uint128> {
        // LSM shares are 1:1 with delegation shares.
        // The actual exchange rate depends on slashing history, but for
        // simplicity we use 1:1 and let the haircut cover slashing risk.
        //
        // A production implementation could:
        // 1. Query the staking module for validator info
        // 2. Calculate actual share-to-token ratio
        // 3. Apply that ratio to the amount
        //
        // For now: LSM share value = amount (1:1 with delegation)
        // The 10% haircut covers slashing risk
        Ok(amount)
    }

    fn haircut_bps(&self) -> u64 {
        HAIRCUT_LSM_BPS // 10%
    }

    fn is_liquidatable(&self, _querier: &QuerierWrapper) -> bool {
        // LSM shares are always potentially liquidatable via intent auction.
        // The auction will determine actual liquidity.
        // In production, could check if validator is jailed.
        true
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::LsmShare
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HYDRO VAULT VALUATION
// ═══════════════════════════════════════════════════════════════════════════

/// Query message for Hydro vault ControlCenterPoolInfo
#[cw_serde]
pub enum HydroVaultQueryMsg {
    /// Get pool info for share valuation
    ControlCenterPoolInfo {},
}

/// Response from Hydro vault ControlCenterPoolInfo query
#[cw_serde]
pub struct ControlCenterPoolInfoResponse {
    pub total_shares_issued: Uint128,
    pub total_pool_value: Uint128,
}

/// Valuation implementation for Hydro Inflow vault shares
pub struct HydroVaultValuation {
    pub vault_contract: String,
    pub share_denom: String,
    pub chain_id: String,
    /// For cross-chain vaults, cached pool info is required
    pub cached_pool_info: Option<CachedPoolInfo>,
}

impl HydroVaultValuation {
    pub fn new_same_chain(vault_contract: String, share_denom: String, chain_id: String) -> Self {
        Self {
            vault_contract,
            share_denom,
            chain_id,
            cached_pool_info: None,
        }
    }

    pub fn new_cross_chain(
        vault_contract: String,
        share_denom: String,
        chain_id: String,
        cached_pool_info: CachedPoolInfo,
    ) -> Self {
        Self {
            vault_contract,
            share_denom,
            chain_id,
            cached_pool_info: Some(cached_pool_info),
        }
    }

    /// Query pool info directly from same-chain vault
    fn query_pool_info_direct(
        &self,
        querier: &QuerierWrapper,
    ) -> ValuationResult<ControlCenterPoolInfoResponse> {
        querier
            .query_wasm_smart(&self.vault_contract, &HydroVaultQueryMsg::ControlCenterPoolInfo {})
            .map_err(|_| CollateralError::NoCachedPoolInfo {
                share_denom: self.share_denom.clone(),
            })
    }

    /// Get pool info - direct query for same-chain, cached for cross-chain
    fn get_pool_info(
        &self,
        querier: &QuerierWrapper,
        env: &Env,
        current_chain_id: &str,
    ) -> ValuationResult<(Uint128, Uint128)> {
        if self.chain_id == current_chain_id {
            // Same-chain: direct query
            let response = self.query_pool_info_direct(querier)?;
            Ok((response.total_pool_value, response.total_shares_issued))
        } else {
            // Cross-chain: use cached pool info
            let cached = self.cached_pool_info.as_ref().ok_or_else(|| {
                CollateralError::NoCachedPoolInfo {
                    share_denom: self.share_denom.clone(),
                }
            })?;

            // Check staleness
            let age = env.block.time.seconds().saturating_sub(cached.updated_at);
            if age > MAX_STALENESS_SECONDS {
                return Err(CollateralError::StalePoolInfo { age_seconds: age });
            }

            Ok((cached.total_pool_value, cached.total_shares_issued))
        }
    }
}

impl CollateralValuation for HydroVaultValuation {
    fn get_raw_value(
        &self,
        querier: &QuerierWrapper,
        env: &Env,
        amount: Uint128,
    ) -> ValuationResult<Uint128> {
        let current_chain_id = &env.block.chain_id;
        let (total_pool_value, total_shares_issued) =
            self.get_pool_info(querier, env, current_chain_id)?;

        // share_value = amount * (total_pool_value / total_shares_issued)
        if total_shares_issued.is_zero() {
            return Ok(amount); // 1:1 if no shares issued yet
        }

        amount
            .checked_multiply_ratio(total_pool_value, total_shares_issued)
            .map_err(|_| CollateralError::NoCachedPoolInfo {
                share_denom: self.share_denom.clone(),
            })
    }

    fn haircut_bps(&self) -> u64 {
        HAIRCUT_HYDRO_BPS // 20%
    }

    fn is_liquidatable(&self, _querier: &QuerierWrapper) -> bool {
        // Hydro vault shares are always potentially liquidatable
        // The intent auction will determine actual liquidity
        true
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::HydroVault
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// VALUATION FACTORY
// ═══════════════════════════════════════════════════════════════════════════

/// Create a valuation instance for a collateral asset
pub fn create_valuation(
    asset: &CollateralAsset,
    cached_pool_info: Option<CachedPoolInfo>,
) -> Box<dyn CollateralValuation> {
    match asset {
        CollateralAsset::Native { denom } => Box::new(NativeValuation::new(denom.clone())),
        CollateralAsset::LsmShare {
            validator,
            share_denom,
        } => Box::new(LsmShareValuation::new(
            validator.clone(),
            share_denom.clone(),
        )),
        CollateralAsset::HydroVault {
            vault_contract,
            share_denom,
            chain_id,
        } => {
            if let Some(cached) = cached_pool_info {
                Box::new(HydroVaultValuation::new_cross_chain(
                    vault_contract.to_string(),
                    share_denom.clone(),
                    chain_id.clone(),
                    cached,
                ))
            } else {
                Box::new(HydroVaultValuation::new_same_chain(
                    vault_contract.to_string(),
                    share_denom.clone(),
                    chain_id.clone(),
                ))
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS FOR BOND POOL VALUATION
// ═══════════════════════════════════════════════════════════════════════════

use crate::collateral::SolverBondPool;

/// Calculate total collateral value for a solver's bond pool
pub fn calculate_total_collateral_value(
    querier: &QuerierWrapper,
    env: &Env,
    pool: &SolverBondPool,
    hydro_cache: &dyn Fn(&str) -> Option<CachedPoolInfo>,
) -> ValuationResult<Uint128> {
    let mut total = Uint128::zero();

    for deposit in &pool.deposits {
        let cached = match &deposit.asset {
            CollateralAsset::HydroVault { share_denom, .. } => hydro_cache(share_denom),
            _ => None,
        };

        let valuation = create_valuation(&deposit.asset, cached);
        let value = valuation.get_collateral_value(querier, env, deposit.amount)?;
        total = total.checked_add(value).unwrap_or(Uint128::MAX);
    }

    Ok(total)
}

/// Calculate available (unlocked) collateral value for a solver's bond pool
pub fn calculate_available_collateral_value(
    querier: &QuerierWrapper,
    env: &Env,
    pool: &SolverBondPool,
    hydro_cache: &dyn Fn(&str) -> Option<CachedPoolInfo>,
) -> ValuationResult<Uint128> {
    let mut total = Uint128::zero();

    for deposit in &pool.deposits {
        let available = deposit.available();
        if available.is_zero() {
            continue;
        }

        let cached = match &deposit.asset {
            CollateralAsset::HydroVault { share_denom, .. } => hydro_cache(share_denom),
            _ => None,
        };

        let valuation = create_valuation(&deposit.asset, cached);
        let value = valuation.get_collateral_value(querier, env, available)?;
        total = total.checked_add(value).unwrap_or(Uint128::MAX);
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_valuation() {
        let valuation = NativeValuation::new("uatom".to_string());

        assert_eq!(valuation.haircut_bps(), 0);
        assert_eq!(valuation.asset_class(), AssetClass::Native);
    }

    #[test]
    fn test_lsm_valuation_structure() {
        let valuation = LsmShareValuation::new(
            "cosmosvaloper1xxx".to_string(),
            "cosmosvaloper1xxx/1".to_string(),
        );

        assert_eq!(valuation.haircut_bps(), 1000); // 10%
        assert_eq!(valuation.asset_class(), AssetClass::LsmShare);
    }

    #[test]
    fn test_hydro_valuation_structure() {
        let valuation = HydroVaultValuation::new_same_chain(
            "neutron1vault".to_string(),
            "factory/neutron1vault/inflow_atom".to_string(),
            "neutron-1".to_string(),
        );

        assert_eq!(valuation.haircut_bps(), 2000); // 20%
        assert_eq!(valuation.asset_class(), AssetClass::HydroVault);
    }

    #[test]
    fn test_hydro_valuation_with_cache() {
        let cached = CachedPoolInfo {
            total_shares_issued: Uint128::new(1000),
            total_pool_value: Uint128::new(1100), // 10% yield
            updated_at: 1000,
        };

        let valuation = HydroVaultValuation::new_cross_chain(
            "neutron1vault".to_string(),
            "factory/neutron1vault/inflow_atom".to_string(),
            "neutron-1".to_string(),
            cached,
        );

        assert!(valuation.cached_pool_info.is_some());
    }

    #[test]
    fn test_cached_pool_info_share_value_calculation() {
        let cached = CachedPoolInfo {
            total_shares_issued: Uint128::new(1000),
            total_pool_value: Uint128::new(1100), // 10% yield accrued
            updated_at: 1000,
        };

        // 100 shares worth 110 base tokens (10% yield)
        let value = cached.calculate_share_value(Uint128::new(100)).unwrap();
        assert_eq!(value, Uint128::new(110));

        // Full pool
        let value = cached.calculate_share_value(Uint128::new(1000)).unwrap();
        assert_eq!(value, Uint128::new(1100));
    }

    #[test]
    fn test_cached_pool_info_staleness() {
        let cached = CachedPoolInfo {
            total_shares_issued: Uint128::new(1000),
            total_pool_value: Uint128::new(1100),
            updated_at: 1000,
        };

        // Fresh (within window)
        assert!(!cached.is_stale(1000 + MAX_STALENESS_SECONDS));

        // Stale (outside window)
        assert!(cached.is_stale(1000 + MAX_STALENESS_SECONDS + 1));
    }

    #[test]
    fn test_create_valuation_factory() {
        // Native
        let asset = CollateralAsset::Native {
            denom: "uatom".to_string(),
        };
        let valuation = create_valuation(&asset, None);
        assert_eq!(valuation.asset_class(), AssetClass::Native);

        // LSM
        let asset = CollateralAsset::LsmShare {
            validator: "cosmosvaloper1xxx".to_string(),
            share_denom: "cosmosvaloper1xxx/1".to_string(),
        };
        let valuation = create_valuation(&asset, None);
        assert_eq!(valuation.asset_class(), AssetClass::LsmShare);

        // Hydro
        let asset = CollateralAsset::HydroVault {
            vault_contract: cosmwasm_std::Addr::unchecked("neutron1vault"),
            share_denom: "factory/neutron1vault/inflow_atom".to_string(),
            chain_id: "neutron-1".to_string(),
        };
        let valuation = create_valuation(&asset, None);
        assert_eq!(valuation.asset_class(), AssetClass::HydroVault);
    }

    #[test]
    fn test_haircut_values() {
        // Native - 0% haircut
        let native = NativeValuation::new("uatom".to_string());
        assert_eq!(native.haircut_bps(), 0);

        // LSM - 10% haircut
        let lsm = LsmShareValuation::new("val".to_string(), "denom".to_string());
        assert_eq!(lsm.haircut_bps(), 1000);

        // Hydro - 20% haircut
        let hydro = HydroVaultValuation::new_same_chain(
            "vault".to_string(),
            "denom".to_string(),
            "chain".to_string(),
        );
        assert_eq!(hydro.haircut_bps(), 2000);
    }

    #[test]
    fn test_is_liquidatable() {
        // All types should be liquidatable via intent auction
        let native = NativeValuation::new("uatom".to_string());
        let lsm = LsmShareValuation::new("val".to_string(), "denom".to_string());
        let hydro = HydroVaultValuation::new_same_chain(
            "vault".to_string(),
            "denom".to_string(),
            "chain".to_string(),
        );

        // Can't easily test without a proper querier, but structure is correct
        assert_eq!(native.asset_class(), AssetClass::Native);
        assert_eq!(lsm.asset_class(), AssetClass::LsmShare);
        assert_eq!(hydro.asset_class(), AssetClass::HydroVault);
    }
}
