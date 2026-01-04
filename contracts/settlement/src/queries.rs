use cosmwasm_std::{Deps, StdResult, Uint128};

use crate::helpers::{reputation_to_response, settlement_to_response, solver_to_response};
use crate::msg::{
    AcceptedLstTokensResponse, BondAssetResponse, ConfigResponse, LsmConfigResponse,
    LstConfigResponse, SettlementResponse, SettlementsResponse, SolverBondResponse,
    SolverReputationResponse, SolverResponse, SolversByReputationResponse, SolversResponse,
    TopSolversResponse,
};
use crate::state::{
    BondAssetType, LsmBondConfig, LstBondConfig, SolverReputation, CONFIG, INTENT_SETTLEMENTS,
    LSM_CONFIG, LST_CONFIG, REPUTATIONS, SETTLEMENTS, SOLVERS,
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
// LSM & LST BOND QUERIES
// ═══════════════════════════════════════════════════════════════════════════

/// Query detailed bond information for a solver
pub fn query_solver_bond(deps: Deps, solver_id: String) -> StdResult<SolverBondResponse> {
    let solver = SOLVERS.load(deps.storage, &solver_id)?;

    // Convert bond assets to response format
    let assets: Vec<BondAssetResponse> = solver
        .bond
        .assets
        .iter()
        .map(|a| {
            let (asset_type, asset_info) = match &a.asset_type {
                BondAssetType::NativeAtom => ("native_atom".to_string(), None),
                BondAssetType::LsmShare { validator } => {
                    ("lsm_share".to_string(), Some(validator.clone()))
                }
                BondAssetType::Lst { protocol } => ("lst".to_string(), Some(protocol.clone())),
            };
            BondAssetResponse {
                denom: a.denom.clone(),
                amount: a.amount,
                asset_type,
                asset_info,
                atom_value: a.atom_value,
            }
        })
        .collect();

    // Calculate category totals
    let native_atom_amount = solver.bond.native_atom_amount();

    let lsm_value: Uint128 = solver
        .bond
        .assets
        .iter()
        .filter(|a| matches!(a.asset_type, BondAssetType::LsmShare { .. }))
        .map(|a| a.atom_value)
        .sum();

    let lst_value: Uint128 = solver
        .bond
        .assets
        .iter()
        .filter(|a| matches!(a.asset_type, BondAssetType::Lst { .. }))
        .map(|a| a.atom_value)
        .sum();

    Ok(SolverBondResponse {
        solver_id,
        assets,
        total_atom_value: solver.bond.total_atom_value,
        native_atom_amount,
        lsm_value,
        lst_value,
        last_updated: solver.bond.last_updated,
    })
}

/// Query LSM bond configuration
pub fn query_lsm_config(deps: Deps) -> StdResult<LsmConfigResponse> {
    let lsm_config = LSM_CONFIG
        .may_load(deps.storage)?
        .unwrap_or_else(LsmBondConfig::default);

    Ok(LsmConfigResponse {
        enabled: lsm_config.enabled,
        blocked_validators: lsm_config.blocked_validators,
        max_lsm_per_solver: lsm_config.max_lsm_per_solver,
        valuation_discount_bps: lsm_config.valuation_discount_bps,
    })
}

/// Query LST bond configuration
pub fn query_lst_config(deps: Deps) -> StdResult<LstConfigResponse> {
    let lst_config = LST_CONFIG
        .may_load(deps.storage)?
        .unwrap_or_else(LstBondConfig::default);

    Ok(LstConfigResponse {
        enabled: lst_config.enabled,
        max_lst_per_solver: lst_config.max_lst_per_solver,
        accepted_tokens_count: lst_config.accepted_tokens.len() as u32,
    })
}

/// Query accepted LST tokens
pub fn query_accepted_lst_tokens(deps: Deps) -> StdResult<AcceptedLstTokensResponse> {
    let lst_config = LST_CONFIG
        .may_load(deps.storage)?
        .unwrap_or_else(LstBondConfig::default);

    Ok(AcceptedLstTokensResponse {
        tokens: lst_config.accepted_tokens,
    })
}
