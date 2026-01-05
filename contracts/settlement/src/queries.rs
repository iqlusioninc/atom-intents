use atom_intents_types::collateral::apply_haircut;
use cosmwasm_std::{Deps, Env, StdResult, Uint128};

use crate::helpers::{reputation_to_response, settlement_to_response, solver_to_response};
use crate::msg::{
    AssetClassValue, AvailableCollateralResponse, BondPoolResponse, CollateralDepositInfo,
    CollateralValueResponse, ConfigResponse, LiquidationResponse, LiquidationsResponse,
    PoolInfoResponse, SettlementResponse, SettlementsResponse, SolverReputationResponse,
    SolverResponse, SolversByReputationResponse, SolversResponse, TopSolversResponse,
};
use crate::state::{
    SolverReputation, BOND_POOLS, CACHED_POOL_INFO, CONFIG, INTENT_SETTLEMENTS,
    LIQUIDATION_INTENTS, REPUTATIONS, SETTLEMENTS, SOLVERS,
};

pub fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse {
        admin: config.admin.to_string(),
        escrow_contract: config.escrow_contract.to_string(),
        min_solver_bond: config.min_solver_bond,
        base_slash_bps: config.base_slash_bps,
    })
}

pub fn query_solver(deps: Deps, solver_id: String) -> StdResult<SolverResponse> {
    let solver = SOLVERS.load(deps.storage, &solver_id)?;
    Ok(solver_to_response(solver))
}

pub fn query_settlement(deps: Deps, settlement_id: String) -> StdResult<SettlementResponse> {
    let settlement = SETTLEMENTS.load(deps.storage, &settlement_id)?;
    Ok(settlement_to_response(settlement))
}

pub fn query_settlement_by_intent(deps: Deps, intent_id: String) -> StdResult<SettlementResponse> {
    let settlement_id = INTENT_SETTLEMENTS.load(deps.storage, &intent_id)?;
    let settlement = SETTLEMENTS.load(deps.storage, &settlement_id)?;
    Ok(settlement_to_response(settlement))
}

pub fn query_solvers(
    deps: Deps,
    _start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<SolversResponse> {
    let limit = limit.unwrap_or(30).min(100) as usize;

    let solvers: Vec<SolverResponse> = SOLVERS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .take(limit)
        .filter_map(|r| r.ok())
        .map(|(_, solver)| solver_to_response(solver))
        .collect();

    Ok(SolversResponse { solvers })
}

pub fn query_settlements_by_solver(
    deps: Deps,
    solver_id: String,
    _start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<SettlementsResponse> {
    let limit = limit.unwrap_or(30).min(100) as usize;

    let settlements: Vec<SettlementResponse> = SETTLEMENTS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .filter_map(|r| r.ok())
        .filter(|(_, s)| s.solver_id == solver_id)
        .take(limit)
        .map(|(_, settlement)| settlement_to_response(settlement))
        .collect();

    Ok(SettlementsResponse { settlements })
}

pub fn query_solver_reputation(deps: Deps, solver_id: String) -> StdResult<SolverReputationResponse> {
    let reputation = REPUTATIONS.load(deps.storage, &solver_id)?;
    Ok(reputation_to_response(reputation))
}

pub fn query_top_solvers(deps: Deps, limit: u32) -> StdResult<TopSolversResponse> {
    let limit = limit.min(100) as usize;

    let mut reputations: Vec<SolverReputation> = REPUTATIONS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .filter_map(|r| r.ok())
        .map(|(_, rep)| rep)
        .collect();

    // Sort by reputation score (descending)
    reputations.sort_by(|a, b| b.reputation_score.cmp(&a.reputation_score));

    let solvers: Vec<SolverReputationResponse> = reputations
        .into_iter()
        .take(limit)
        .map(reputation_to_response)
        .collect();

    Ok(TopSolversResponse { solvers })
}

pub fn query_solvers_by_reputation(
    deps: Deps,
    min_score: u64,
    limit: u32,
) -> StdResult<SolversByReputationResponse> {
    let limit = limit.min(100) as usize;

    let mut reputations: Vec<SolverReputation> = REPUTATIONS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .filter_map(|r| r.ok())
        .map(|(_, rep)| rep)
        .filter(|rep| rep.reputation_score >= min_score)
        .collect();

    // Sort by reputation score (descending)
    reputations.sort_by(|a, b| b.reputation_score.cmp(&a.reputation_score));

    let solvers: Vec<SolverReputationResponse> = reputations
        .into_iter()
        .take(limit)
        .map(reputation_to_response)
        .collect();

    Ok(SolversByReputationResponse { solvers })
}

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL QUERIES
// ═══════════════════════════════════════════════════════════════════════════

pub fn query_bond_pool(deps: Deps, solver_id: String) -> StdResult<BondPoolResponse> {
    let pool = BOND_POOLS.may_load(deps.storage, &solver_id)?;

    let deposits = match pool {
        Some(p) => p
            .deposits
            .into_iter()
            .map(|d| CollateralDepositInfo {
                asset_class: d.asset.asset_class(),
                haircut_bps: d.asset.haircut_bps(),
                asset: d.asset,
                total_amount: d.amount,
                locked_amount: d.locked_amount,
                available_amount: d.amount.saturating_sub(d.locked_amount),
            })
            .collect(),
        None => vec![],
    };

    Ok(BondPoolResponse { solver_id, deposits })
}

pub fn query_collateral_value(
    deps: Deps,
    env: Env,
    solver_id: String,
) -> StdResult<CollateralValueResponse> {
    use atom_intents_types::collateral::CollateralAsset;

    let pool = BOND_POOLS.may_load(deps.storage, &solver_id)?;

    let mut total_value = Uint128::zero();
    let mut by_class: Vec<AssetClassValue> = vec![];

    if let Some(p) = pool {
        for deposit in p.deposits {
            let raw_value = match &deposit.asset {
                CollateralAsset::Native { denom } => {
                    if denom == "uatom" {
                        deposit.amount
                    } else {
                        Uint128::zero() // Non-ATOM natives not valued
                    }
                }
                CollateralAsset::LsmShare { .. } => deposit.amount, // 1:1 with ATOM
                CollateralAsset::HydroVault { share_denom, .. } => {
                    if let Ok(info) = CACHED_POOL_INFO.load(deps.storage, share_denom) {
                        if !info.is_stale(env.block.time.seconds()) {
                            info.calculate_share_value(deposit.amount).unwrap_or(Uint128::zero())
                        } else {
                            Uint128::zero()
                        }
                    } else {
                        Uint128::zero()
                    }
                }
            };

            let haircut_bps = deposit.asset.haircut_bps();
            let value_after_haircut = apply_haircut(raw_value, haircut_bps);

            total_value = total_value.checked_add(value_after_haircut).unwrap_or(total_value);

            by_class.push(AssetClassValue {
                class: deposit.asset.asset_class(),
                raw_value,
                haircut_bps,
                value_after_haircut,
            });
        }
    }

    Ok(CollateralValueResponse {
        solver_id,
        total_value,
        by_class,
    })
}

pub fn query_available_collateral(
    deps: Deps,
    env: Env,
    solver_id: String,
) -> StdResult<AvailableCollateralResponse> {
    use atom_intents_types::collateral::CollateralAsset;

    let pool = BOND_POOLS.may_load(deps.storage, &solver_id)?;

    let mut available_value = Uint128::zero();

    if let Some(p) = pool {
        for deposit in p.deposits {
            let available = deposit.amount.saturating_sub(deposit.locked_amount);

            let raw_value = match &deposit.asset {
                CollateralAsset::Native { denom } => {
                    if denom == "uatom" {
                        available
                    } else {
                        Uint128::zero()
                    }
                }
                CollateralAsset::LsmShare { .. } => available,
                CollateralAsset::HydroVault { share_denom, .. } => {
                    if let Ok(info) = CACHED_POOL_INFO.load(deps.storage, share_denom) {
                        if !info.is_stale(env.block.time.seconds()) {
                            info.calculate_share_value(available).unwrap_or(Uint128::zero())
                        } else {
                            Uint128::zero()
                        }
                    } else {
                        Uint128::zero()
                    }
                }
            };

            let value_after_haircut = apply_haircut(raw_value, deposit.asset.haircut_bps());
            available_value = available_value.checked_add(value_after_haircut).unwrap_or(available_value);
        }
    }

    Ok(AvailableCollateralResponse {
        solver_id,
        available_value,
        can_accept_settlements: !available_value.is_zero(),
    })
}

pub fn query_liquidation(deps: Deps, liquidation_id: String) -> StdResult<LiquidationResponse> {
    let intent = LIQUIDATION_INTENTS.load(deps.storage, &liquidation_id)?;

    Ok(LiquidationResponse {
        id: intent.id,
        source_settlement_id: intent.source_settlement_id,
        slashed_solver: intent.slashed_solver,
        collateral_asset: intent.collateral.asset,
        collateral_amount: intent.collateral.amount,
        min_output_amount: intent.min_output_amount,
        beneficiary: intent.beneficiary.to_string(),
        timeout: intent.timeout,
        status: intent.status,
        winning_bid: None, // Would need to track bids separately
    })
}

pub fn query_pending_liquidations(
    deps: Deps,
    _start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<LiquidationsResponse> {
    use atom_intents_types::collateral::LiquidationStatus;

    let limit = limit.unwrap_or(30).min(100) as usize;

    let liquidations: Vec<LiquidationResponse> = LIQUIDATION_INTENTS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .filter_map(|r| r.ok())
        .filter(|(_, intent)| {
            matches!(
                intent.status,
                LiquidationStatus::Pending | LiquidationStatus::Auctioning
            )
        })
        .take(limit)
        .map(|(_, intent)| LiquidationResponse {
            id: intent.id,
            source_settlement_id: intent.source_settlement_id,
            slashed_solver: intent.slashed_solver,
            collateral_asset: intent.collateral.asset,
            collateral_amount: intent.collateral.amount,
            min_output_amount: intent.min_output_amount,
            beneficiary: intent.beneficiary.to_string(),
            timeout: intent.timeout,
            status: intent.status,
            winning_bid: None,
        })
        .collect();

    Ok(LiquidationsResponse { liquidations })
}

pub fn query_cached_pool_info(deps: Deps, env: Env, share_denom: String) -> StdResult<PoolInfoResponse> {
    let info = CACHED_POOL_INFO.load(deps.storage, &share_denom)?;

    Ok(PoolInfoResponse {
        share_denom,
        total_shares_issued: info.total_shares_issued,
        total_pool_value: info.total_pool_value,
        updated_at: info.updated_at,
        is_stale: info.is_stale(env.block.time.seconds()),
    })
}
