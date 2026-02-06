use cosmwasm_std::{Deps, StdResult};
use cw_storage_plus::Bound;

use crate::helpers::{reputation_to_response, settlement_to_response, solver_to_response};
use crate::msg::{
    ConfigResponse, OrderResponse, OrdersResponse, SettlementResponse, SettlementsResponse,
    SolverReputationResponse, SolverResponse, SolversByReputationResponse, SolversResponse,
    TopSolversResponse,
};
use crate::state::{
    Order, OrderStatus, SolverReputation, CONFIG, INTENT_SETTLEMENTS, ORDERS, REPUTATIONS,
    SETTLEMENTS, SOLVERS, USER_ORDERS,
};

pub fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse {
        admin: config.admin.to_string(),
        escrow_contract: config.escrow_contract.to_string(),
        allowed_ibc_channels: config.allowed_ibc_channels,
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
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<SolversResponse> {
    let limit = limit.unwrap_or(30).min(100) as usize;
    let start = start_after.as_deref().map(Bound::exclusive);

    let solvers: Vec<SolverResponse> = SOLVERS
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
        .take(limit)
        .filter_map(|r| r.ok())
        .map(|(_, solver)| solver_to_response(solver))
        .collect();

    Ok(SolversResponse { solvers })
}

pub fn query_settlements_by_solver(
    deps: Deps,
    solver_id: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<SettlementsResponse> {
    let limit = limit.unwrap_or(30).min(100) as usize;
    let start = start_after.as_deref().map(Bound::exclusive);

    let settlements: Vec<SettlementResponse> = SETTLEMENTS
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
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
// ON-CHAIN ORDER QUERIES
// ═══════════════════════════════════════════════════════════════════════════

fn order_to_response(order: Order) -> OrderResponse {
    OrderResponse {
        id: order.id,
        user: order.user.to_string(),
        input_amount: order.input_amount,
        input_denom: order.input_denom,
        min_output_amount: order.min_output_amount,
        output_denom: order.output_denom,
        destination_chain: order.destination_chain,
        recipient: order.recipient,
        status: order.status.as_str().to_string(),
        created_at: order.created_at,
        expires_at: order.expires_at,
        settlement_id: order.settlement_id,
    }
}

pub fn query_order(deps: Deps, order_id: String) -> StdResult<OrderResponse> {
    let order = ORDERS.load(deps.storage, &order_id)?;
    Ok(order_to_response(order))
}

pub fn query_open_orders(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<OrdersResponse> {
    let limit = limit.unwrap_or(30).min(100) as usize;

    let start = start_after.as_deref().map(Bound::exclusive);

    let orders: Vec<OrderResponse> = ORDERS
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
        .filter_map(|r| r.ok())
        .filter(|(_, order)| matches!(order.status, OrderStatus::Open))
        .take(limit)
        .map(|(_, order)| order_to_response(order))
        .collect();

    let total = orders.len() as u64;

    Ok(OrdersResponse { orders, total })
}

pub fn query_orders_by_user(
    deps: Deps,
    user: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<OrdersResponse> {
    let limit = limit.unwrap_or(30).min(100) as usize;
    let user_addr = deps.api.addr_validate(&user)?;

    let start = start_after.as_deref().map(Bound::exclusive);

    // Get order IDs from the user index
    let order_ids: Vec<String> = USER_ORDERS
        .prefix(&user_addr)
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
        .take(limit)
        .filter_map(|r| r.ok())
        .map(|(order_id, _)| order_id)
        .collect();

    // Load each order
    let orders: Vec<OrderResponse> = order_ids
        .into_iter()
        .filter_map(|order_id| ORDERS.load(deps.storage, &order_id).ok())
        .map(order_to_response)
        .collect();

    let total = orders.len() as u64;

    Ok(OrdersResponse { orders, total })
}
