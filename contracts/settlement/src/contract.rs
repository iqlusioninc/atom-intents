use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Response, StdResult, Uint128,
};

use crate::error::ContractError;
use crate::msg::{
    ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg, SettlementResponse,
    SettlementsResponse, SolverResponse, SolversResponse,
};
use crate::state::{
    Config, RegisteredSolver, Settlement, SettlementStatus, CONFIG, INTENT_SETTLEMENTS,
    SETTLEMENTS, SOLVERS,
};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config = Config {
        admin: deps.api.addr_validate(&msg.admin)?,
        escrow_contract: deps.api.addr_validate(&msg.escrow_contract)?,
        min_solver_bond: msg.min_solver_bond,
        base_slash_bps: msg.base_slash_bps,
    };
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "instantiate"))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::RegisterSolver { solver_id } => {
            execute_register_solver(deps, env, info, solver_id)
        }
        ExecuteMsg::DeregisterSolver { solver_id } => {
            execute_deregister_solver(deps, info, solver_id)
        }
        ExecuteMsg::CreateSettlement {
            settlement_id,
            intent_id,
            solver_id,
            user,
            user_input_amount,
            user_input_denom,
            solver_output_amount,
            solver_output_denom,
            expires_at,
        } => execute_create_settlement(
            deps,
            env,
            info,
            settlement_id,
            intent_id,
            solver_id,
            user,
            user_input_amount,
            user_input_denom,
            solver_output_amount,
            solver_output_denom,
            expires_at,
        ),
        ExecuteMsg::MarkUserLocked {
            settlement_id,
            escrow_id,
        } => execute_mark_user_locked(deps, info, settlement_id, escrow_id),
        ExecuteMsg::MarkSolverLocked { settlement_id } => {
            execute_mark_solver_locked(deps, info, settlement_id)
        }
        ExecuteMsg::MarkExecuting { settlement_id } => {
            execute_mark_executing(deps, info, settlement_id)
        }
        ExecuteMsg::MarkCompleted { settlement_id } => {
            execute_mark_completed(deps, info, settlement_id)
        }
        ExecuteMsg::MarkFailed {
            settlement_id,
            reason,
        } => execute_mark_failed(deps, info, settlement_id, reason),
        ExecuteMsg::SlashSolver {
            solver_id,
            settlement_id,
        } => execute_slash_solver(deps, info, solver_id, settlement_id),
        ExecuteMsg::UpdateConfig {
            admin,
            escrow_contract,
            min_solver_bond,
            base_slash_bps,
        } => execute_update_config(
            deps,
            info,
            admin,
            escrow_contract,
            min_solver_bond,
            base_slash_bps,
        ),
    }
}

fn execute_register_solver(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    solver_id: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Check bond amount
    let bond_amount: Uint128 = info
        .funds
        .iter()
        .filter(|c| c.denom == "uatom")
        .map(|c| c.amount)
        .sum();

    if bond_amount < config.min_solver_bond {
        return Err(ContractError::InsufficientBond {
            required: config.min_solver_bond.to_string(),
            provided: bond_amount.to_string(),
        });
    }

    let solver = RegisteredSolver {
        id: solver_id.clone(),
        operator: info.sender.clone(),
        bond_amount,
        active: true,
        total_settlements: 0,
        failed_settlements: 0,
        registered_at: env.block.time.seconds(),
    };

    SOLVERS.save(deps.storage, &solver_id, &solver)?;

    Ok(Response::new()
        .add_attribute("action", "register_solver")
        .add_attribute("solver_id", solver_id)
        .add_attribute("operator", info.sender)
        .add_attribute("bond_amount", bond_amount))
}

fn execute_deregister_solver(
    deps: DepsMut,
    info: MessageInfo,
    solver_id: String,
) -> Result<Response, ContractError> {
    let solver = SOLVERS
        .load(deps.storage, &solver_id)
        .map_err(|_| ContractError::SolverNotRegistered { id: solver_id.clone() })?;

    // Only operator can deregister
    if info.sender != solver.operator {
        return Err(ContractError::Unauthorized {});
    }

    // Return bond
    let send_msg = BankMsg::Send {
        to_address: solver.operator.to_string(),
        amount: vec![Coin {
            denom: "uatom".to_string(),
            amount: solver.bond_amount,
        }],
    };

    SOLVERS.remove(deps.storage, &solver_id);

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "deregister_solver")
        .add_attribute("solver_id", solver_id)
        .add_attribute("bond_returned", solver.bond_amount))
}

fn execute_create_settlement(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    settlement_id: String,
    intent_id: String,
    solver_id: String,
    user: String,
    user_input_amount: Uint128,
    user_input_denom: String,
    solver_output_amount: Uint128,
    solver_output_denom: String,
    expires_at: u64,
) -> Result<Response, ContractError> {
    // Verify solver exists
    if !SOLVERS.has(deps.storage, &solver_id) {
        return Err(ContractError::SolverNotRegistered { id: solver_id });
    }

    // Check settlement doesn't exist
    if SETTLEMENTS.has(deps.storage, &settlement_id) {
        return Err(ContractError::SettlementAlreadyExists { id: settlement_id });
    }

    let settlement = Settlement {
        id: settlement_id.clone(),
        intent_id: intent_id.clone(),
        solver_id,
        user: deps.api.addr_validate(&user)?,
        user_input_amount,
        user_input_denom,
        solver_output_amount,
        solver_output_denom,
        status: SettlementStatus::Pending,
        created_at: env.block.time.seconds(),
        expires_at,
        escrow_id: None,
    };

    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;
    INTENT_SETTLEMENTS.save(deps.storage, &intent_id, &settlement_id)?;

    Ok(Response::new()
        .add_attribute("action", "create_settlement")
        .add_attribute("settlement_id", settlement_id)
        .add_attribute("intent_id", intent_id))
}

fn execute_mark_user_locked(
    deps: DepsMut,
    info: MessageInfo,
    settlement_id: String,
    escrow_id: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only escrow contract can call this
    if info.sender != config.escrow_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound { id: settlement_id.clone() })?;

    settlement.status = SettlementStatus::UserLocked;
    settlement.escrow_id = Some(escrow_id);
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_user_locked")
        .add_attribute("settlement_id", settlement_id))
}

fn execute_mark_solver_locked(
    deps: DepsMut,
    _info: MessageInfo,
    settlement_id: String,
) -> Result<Response, ContractError> {
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound { id: settlement_id.clone() })?;

    settlement.status = SettlementStatus::SolverLocked;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_solver_locked")
        .add_attribute("settlement_id", settlement_id))
}

fn execute_mark_executing(
    deps: DepsMut,
    _info: MessageInfo,
    settlement_id: String,
) -> Result<Response, ContractError> {
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound { id: settlement_id.clone() })?;

    settlement.status = SettlementStatus::Executing;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_executing")
        .add_attribute("settlement_id", settlement_id))
}

fn execute_mark_completed(
    deps: DepsMut,
    _info: MessageInfo,
    settlement_id: String,
) -> Result<Response, ContractError> {
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound { id: settlement_id.clone() })?;

    // Update solver stats
    let mut solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
    solver.total_settlements += 1;
    SOLVERS.save(deps.storage, &settlement.solver_id, &solver)?;

    settlement.status = SettlementStatus::Completed;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_completed")
        .add_attribute("settlement_id", settlement_id))
}

fn execute_mark_failed(
    deps: DepsMut,
    _info: MessageInfo,
    settlement_id: String,
    reason: String,
) -> Result<Response, ContractError> {
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound { id: settlement_id.clone() })?;

    // Update solver stats
    let mut solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
    solver.total_settlements += 1;
    solver.failed_settlements += 1;
    SOLVERS.save(deps.storage, &settlement.solver_id, &solver)?;

    settlement.status = SettlementStatus::Failed {
        reason: reason.clone(),
    };
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_failed")
        .add_attribute("settlement_id", settlement_id)
        .add_attribute("reason", reason))
}

fn execute_slash_solver(
    deps: DepsMut,
    info: MessageInfo,
    solver_id: String,
    settlement_id: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only admin can slash
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut solver = SOLVERS
        .load(deps.storage, &solver_id)
        .map_err(|_| ContractError::SolverNotRegistered { id: solver_id.clone() })?;

    let settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound { id: settlement_id.clone() })?;

    // Calculate slash amount (base_slash_bps of settlement value)
    let slash_amount = settlement.user_input_amount * Uint128::from(config.base_slash_bps)
        / Uint128::from(10000u64);
    let actual_slash = std::cmp::min(slash_amount, solver.bond_amount);

    solver.bond_amount = solver.bond_amount.saturating_sub(actual_slash);
    SOLVERS.save(deps.storage, &solver_id, &solver)?;

    // Update settlement status
    let mut settlement = settlement;
    settlement.status = SettlementStatus::Slashed {
        amount: actual_slash,
    };
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "slash_solver")
        .add_attribute("solver_id", solver_id)
        .add_attribute("settlement_id", settlement_id)
        .add_attribute("slash_amount", actual_slash))
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    admin: Option<String>,
    escrow_contract: Option<String>,
    min_solver_bond: Option<Uint128>,
    base_slash_bps: Option<u64>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(admin) = admin {
        config.admin = deps.api.addr_validate(&admin)?;
    }
    if let Some(escrow_contract) = escrow_contract {
        config.escrow_contract = deps.api.addr_validate(&escrow_contract)?;
    }
    if let Some(min_solver_bond) = min_solver_bond {
        config.min_solver_bond = min_solver_bond;
    }
    if let Some(base_slash_bps) = base_slash_bps {
        config.base_slash_bps = base_slash_bps;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Solver { solver_id } => to_json_binary(&query_solver(deps, solver_id)?),
        QueryMsg::Settlement { settlement_id } => {
            to_json_binary(&query_settlement(deps, settlement_id)?)
        }
        QueryMsg::SettlementByIntent { intent_id } => {
            to_json_binary(&query_settlement_by_intent(deps, intent_id)?)
        }
        QueryMsg::Solvers { start_after, limit } => {
            to_json_binary(&query_solvers(deps, start_after, limit)?)
        }
        QueryMsg::SettlementsBySolver {
            solver_id,
            start_after,
            limit,
        } => to_json_binary(&query_settlements_by_solver(
            deps,
            solver_id,
            start_after,
            limit,
        )?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse {
        admin: config.admin.to_string(),
        escrow_contract: config.escrow_contract.to_string(),
        min_solver_bond: config.min_solver_bond,
        base_slash_bps: config.base_slash_bps,
    })
}

fn query_solver(deps: Deps, solver_id: String) -> StdResult<SolverResponse> {
    let solver = SOLVERS.load(deps.storage, &solver_id)?;
    Ok(solver_to_response(solver))
}

fn query_settlement(deps: Deps, settlement_id: String) -> StdResult<SettlementResponse> {
    let settlement = SETTLEMENTS.load(deps.storage, &settlement_id)?;
    Ok(settlement_to_response(settlement))
}

fn query_settlement_by_intent(deps: Deps, intent_id: String) -> StdResult<SettlementResponse> {
    let settlement_id = INTENT_SETTLEMENTS.load(deps.storage, &intent_id)?;
    let settlement = SETTLEMENTS.load(deps.storage, &settlement_id)?;
    Ok(settlement_to_response(settlement))
}

fn query_solvers(
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

fn query_settlements_by_solver(
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

fn solver_to_response(solver: RegisteredSolver) -> SolverResponse {
    SolverResponse {
        id: solver.id,
        operator: solver.operator.to_string(),
        bond_amount: solver.bond_amount,
        active: solver.active,
        total_settlements: solver.total_settlements,
        failed_settlements: solver.failed_settlements,
        registered_at: solver.registered_at,
    }
}

fn settlement_to_response(settlement: Settlement) -> SettlementResponse {
    let status = match settlement.status {
        SettlementStatus::Pending => "pending".to_string(),
        SettlementStatus::UserLocked => "user_locked".to_string(),
        SettlementStatus::SolverLocked => "solver_locked".to_string(),
        SettlementStatus::Executing => "executing".to_string(),
        SettlementStatus::Completed => "completed".to_string(),
        SettlementStatus::Failed { reason } => format!("failed: {}", reason),
        SettlementStatus::Slashed { amount } => format!("slashed: {}", amount),
    };

    SettlementResponse {
        id: settlement.id,
        intent_id: settlement.intent_id,
        solver_id: settlement.solver_id,
        user: settlement.user.to_string(),
        user_input_amount: settlement.user_input_amount,
        user_input_denom: settlement.user_input_denom,
        solver_output_amount: settlement.solver_output_amount,
        solver_output_denom: settlement.solver_output_denom,
        status,
        created_at: settlement.created_at,
        expires_at: settlement.expires_at,
    }
}
