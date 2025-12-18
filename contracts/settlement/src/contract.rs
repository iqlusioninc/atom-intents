use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, IbcMsg, IbcTimeout,
    MessageInfo, Response, StdResult, Uint128, WasmMsg,
};

use crate::error::ContractError;
use crate::msg::{
    ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg, SettlementResponse, SettlementsResponse,
    SolverReputationResponse, SolverResponse, SolversByReputationResponse, SolversResponse,
    TopSolversResponse,
};
use crate::state::{
    Config, FeeTier, RegisteredSolver, Settlement, SettlementStatus, SolverReputation, CONFIG,
    INTENT_SETTLEMENTS, REPUTATIONS, SETTLEMENTS, SOLVERS,
};

// Escrow contract execute messages
#[cosmwasm_schema::cw_serde]
pub enum EscrowExecuteMsg {
    Release {
        escrow_id: String,
        recipient: String,
    },
    Refund {
        escrow_id: String,
    },
}

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
        ExecuteMsg::ExecuteSettlement {
            settlement_id,
            ibc_channel,
        } => execute_settlement(deps, env, info, settlement_id, ibc_channel),
        ExecuteMsg::HandleTimeout { settlement_id } => {
            execute_handle_timeout(deps, env, info, settlement_id)
        }
        ExecuteMsg::HandleIbcAck {
            settlement_id,
            success,
        } => execute_handle_ibc_ack(deps, info, settlement_id, success),
        ExecuteMsg::UpdateReputation { solver_id } => {
            execute_update_reputation(deps, env, solver_id)
        }
        ExecuteMsg::DecayReputation { start_after, limit } => {
            execute_decay_reputation(deps, env, start_after, limit)
        }
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
    let solver =
        SOLVERS
            .load(deps.storage, &solver_id)
            .map_err(|_| ContractError::SolverNotRegistered {
                id: solver_id.clone(),
            })?;

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
    info: MessageInfo,
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
    // Verify solver exists and sender is authorized
    let solver = SOLVERS.load(deps.storage, &solver_id)
        .map_err(|_| ContractError::SolverNotRegistered { id: solver_id.clone() })?;

    if info.sender != solver.operator {
        return Err(ContractError::Unauthorized {});
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
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    settlement.status = SettlementStatus::UserLocked;
    settlement.escrow_id = Some(escrow_id);
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_user_locked")
        .add_attribute("settlement_id", settlement_id))
}

fn execute_mark_solver_locked(
    deps: DepsMut,
    info: MessageInfo,
    settlement_id: String,
) -> Result<Response, ContractError> {
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    // Only the solver's operator can call this
    let solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
    if info.sender != solver.operator {
        return Err(ContractError::Unauthorized {});
    }

    settlement.status = SettlementStatus::SolverLocked;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_solver_locked")
        .add_attribute("settlement_id", settlement_id))
}

fn execute_mark_executing(
    deps: DepsMut,
    info: MessageInfo,
    settlement_id: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    // Only admin or the solver's operator can call this
    let solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
    if info.sender != config.admin && info.sender != solver.operator {
        return Err(ContractError::Unauthorized {});
    }

    settlement.status = SettlementStatus::Executing;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_executing")
        .add_attribute("settlement_id", settlement_id))
}

fn execute_mark_completed(
    deps: DepsMut,
    info: MessageInfo,
    settlement_id: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    // Only admin can call this (IBC callbacks would go through admin or contract itself)
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

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
    info: MessageInfo,
    settlement_id: String,
    reason: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    // Only admin can call this (IBC timeout callbacks would go through admin)
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

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

    let mut solver =
        SOLVERS
            .load(deps.storage, &solver_id)
            .map_err(|_| ContractError::SolverNotRegistered {
                id: solver_id.clone(),
            })?;

    let settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

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

fn execute_settlement(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    settlement_id: String,
    ibc_channel: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    // Only admin or the solver's operator can call this
    let solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
    if info.sender != config.admin && info.sender != solver.operator {
        return Err(ContractError::Unauthorized {});
    }

    // Verify settlement is in correct state (SolverLocked means both parties ready)
    match &settlement.status {
        SettlementStatus::SolverLocked => {}
        _ => {
            return Err(ContractError::InvalidStateTransition {
                from: format!("{:?}", settlement.status),
                to: "Executing".to_string(),
            });
        }
    }

    // Check not expired
    if env.block.time.seconds() > settlement.expires_at {
        return Err(ContractError::SettlementExpired {});
    }

    // Update status to executing
    settlement.status = SettlementStatus::Executing;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    // Create IBC transfer message to send solver output to user
    // This is the actual on-chain IBC transfer that ensures trustless execution
    let ibc_transfer = IbcMsg::Transfer {
        channel_id: ibc_channel.clone(),
        to_address: settlement.user.to_string(),
        amount: Coin {
            denom: settlement.solver_output_denom.clone(),
            amount: settlement.solver_output_amount,
        },
        timeout: IbcTimeout::with_timestamp(env.block.time.plus_seconds(600)), // 10 min timeout
        memo: Some(format!("ATOM Intent Settlement {}", settlement_id)),
    };

    Ok(Response::new()
        .add_message(ibc_transfer)
        .add_attribute("action", "execute_settlement")
        .add_attribute("settlement_id", settlement_id)
        .add_attribute("ibc_channel", ibc_channel)
        .add_attribute("recipient", settlement.user.to_string())
        .add_attribute("amount", settlement.solver_output_amount.to_string())
        .add_attribute("denom", settlement.solver_output_denom))
}

fn execute_handle_timeout(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    settlement_id: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only admin can call this (IBC module would typically call via admin or a relayer with admin privileges)
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    // Can only timeout if in Executing state
    match &settlement.status {
        SettlementStatus::Executing => {}
        _ => {
            return Err(ContractError::InvalidStateTransition {
                from: format!("{:?}", settlement.status),
                to: "Timeout".to_string(),
            });
        }
    }

    // Get escrow_id before marking as failed
    let escrow_id =
        settlement
            .escrow_id
            .clone()
            .ok_or_else(|| ContractError::InvalidStateTransition {
                from: "Executing".to_string(),
                to: "No escrow_id found".to_string(),
            })?;

    // Mark as failed due to timeout
    settlement.status = SettlementStatus::Failed {
        reason: "IBC transfer timeout".to_string(),
    };
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    // Update solver stats
    let mut solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
    solver.total_settlements += 1;
    solver.failed_settlements += 1;
    SOLVERS.save(deps.storage, &settlement.solver_id, &solver)?;

    // Create refund message to escrow contract
    let refund_msg = WasmMsg::Execute {
        contract_addr: config.escrow_contract.to_string(),
        msg: to_json_binary(&EscrowExecuteMsg::Refund {
            escrow_id: escrow_id.clone(),
        })?,
        funds: vec![],
    };

    Ok(Response::new()
        .add_message(refund_msg)
        .add_attribute("action", "handle_timeout")
        .add_attribute("settlement_id", settlement_id)
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("refund_user", settlement.user.to_string())
        .add_attribute("refund_amount", settlement.user_input_amount.to_string())
        .add_attribute("refund_denom", settlement.user_input_denom))
}

fn execute_handle_ibc_ack(
    deps: DepsMut,
    info: MessageInfo,
    settlement_id: String,
    success: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only admin can call this (IBC module would typically call via admin or a relayer with admin privileges)
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    // Can only handle ack if in Executing state
    match &settlement.status {
        SettlementStatus::Executing => {}
        _ => {
            return Err(ContractError::InvalidStateTransition {
                from: format!("{:?}", settlement.status),
                to: if success { "Completed" } else { "Failed" }.to_string(),
            });
        }
    }

    // Get escrow_id
    let escrow_id =
        settlement
            .escrow_id
            .clone()
            .ok_or_else(|| ContractError::InvalidStateTransition {
                from: "Executing".to_string(),
                to: "No escrow_id found".to_string(),
            })?;

    if success {
        // Update solver stats
        let mut solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
        solver.total_settlements += 1;
        SOLVERS.save(deps.storage, &settlement.solver_id, &solver)?;

        settlement.status = SettlementStatus::Completed;
        SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

        // Get solver's operator address to receive the funds
        let solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
        let solver_address = solver.operator.to_string();

        // Create release message to escrow contract
        let release_msg = WasmMsg::Execute {
            contract_addr: config.escrow_contract.to_string(),
            msg: to_json_binary(&EscrowExecuteMsg::Release {
                escrow_id: escrow_id.clone(),
                recipient: solver_address.clone(),
            })?,
            funds: vec![],
        };

        Ok(Response::new()
            .add_message(release_msg)
            .add_attribute("action", "handle_ibc_ack")
            .add_attribute("settlement_id", settlement_id)
            .add_attribute("result", "success")
            .add_attribute("escrow_id", escrow_id)
            .add_attribute("release_to_solver", solver_address)
            .add_attribute("release_amount", settlement.user_input_amount.to_string())
            .add_attribute("release_denom", settlement.user_input_denom))
    } else {
        // Update solver stats for failure
        let mut solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
        solver.total_settlements += 1;
        solver.failed_settlements += 1;
        SOLVERS.save(deps.storage, &settlement.solver_id, &solver)?;

        settlement.status = SettlementStatus::Failed {
            reason: "IBC transfer failed".to_string(),
        };
        SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

        // Create refund message to escrow contract
        let refund_msg = WasmMsg::Execute {
            contract_addr: config.escrow_contract.to_string(),
            msg: to_json_binary(&EscrowExecuteMsg::Refund {
                escrow_id: escrow_id.clone(),
            })?,
            funds: vec![],
        };

        Ok(Response::new()
            .add_message(refund_msg)
            .add_attribute("action", "handle_ibc_ack")
            .add_attribute("settlement_id", settlement_id)
            .add_attribute("result", "failure")
            .add_attribute("escrow_id", escrow_id)
            .add_attribute("refund_user", settlement.user.to_string())
            .add_attribute("refund_amount", settlement.user_input_amount.to_string())
            .add_attribute("refund_denom", settlement.user_input_denom))
    }
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
        QueryMsg::SolverReputation { solver_id } => {
            to_json_binary(&query_solver_reputation(deps, solver_id)?)
        }
        QueryMsg::TopSolvers { limit } => to_json_binary(&query_top_solvers(deps, limit)?),
        QueryMsg::SolversByReputation { min_score, limit } => {
            to_json_binary(&query_solvers_by_reputation(deps, min_score, limit)?)
        }
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

// ==================== REPUTATION FUNCTIONS ====================

fn calculate_reputation_score(rep: &SolverReputation) -> u64 {
    // Weighted formula:
    // - Success rate: 40%
    // - Volume: 20%
    // - Speed: 20%
    // - No slashing: 20%

    if rep.total_settlements == 0 {
        return 5000; // Default starting score for new solvers
    }

    // Success rate component (0-4000 points, 40%)
    let success_rate = if rep.total_settlements > 0 {
        (rep.successful_settlements * 4000) / rep.total_settlements
    } else {
        0
    };

    // Volume component (0-2000 points, 20%)
    // Scale: 0-10M = 0-2000 points (linear)
    let volume_score = {
        let volume_u64 = rep.total_volume.u128().min(10_000_000) as u64;
        (volume_u64 * 2000) / 10_000_000
    };

    // Speed component (0-2000 points, 20%)
    // Faster settlements get higher scores
    // Assume ideal settlement time is 60 seconds, max acceptable is 300 seconds
    let speed_score = if rep.average_settlement_time == 0 {
        2000
    } else if rep.average_settlement_time <= 60 {
        2000
    } else if rep.average_settlement_time >= 300 {
        0
    } else {
        2000 - ((rep.average_settlement_time - 60) * 2000) / 240
    };

    // No slashing component (0-2000 points, 20%)
    // Each slashing event reduces score
    let slash_penalty = rep.slashing_events.min(10) * 200; // -200 points per slash, max -2000
    let slash_score = 2000u64.saturating_sub(slash_penalty);

    // Total score
    let total = success_rate + volume_score + speed_score + slash_score;
    total.min(10000) // Cap at 10000
}

fn get_solver_fee_tier(score: u64) -> FeeTier {
    match score {
        9000..=10000 => FeeTier::Premium,
        7000..=8999 => FeeTier::Standard,
        5000..=6999 => FeeTier::Basic,
        _ => FeeTier::New,
    }
}

fn execute_update_reputation(
    deps: DepsMut,
    env: Env,
    solver_id: String,
) -> Result<Response, ContractError> {
    // Load solver to ensure it exists
    let _solver =
        SOLVERS
            .load(deps.storage, &solver_id)
            .map_err(|_| ContractError::SolverNotRegistered {
                id: solver_id.clone(),
            })?;

    // Get all settlements for this solver
    let settlements: Vec<Settlement> = SETTLEMENTS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .filter_map(|r| r.ok())
        .filter(|(_, s)| s.solver_id == solver_id)
        .map(|(_, s)| s)
        .collect();

    // Calculate reputation metrics
    let mut successful = 0u64;
    let mut failed = 0u64;
    let mut total_volume = Uint128::zero();
    let mut total_time = 0u64;
    let mut slashing_events = 0u64;
    let mut completed_count = 0u64;

    for settlement in &settlements {
        match &settlement.status {
            SettlementStatus::Completed => {
                successful += 1;
                total_volume += settlement.user_input_amount;
                // Calculate settlement time
                let settlement_time = if settlement.created_at < env.block.time.seconds() {
                    env.block.time.seconds() - settlement.created_at
                } else {
                    0
                };
                total_time += settlement_time;
                completed_count += 1;
            }
            SettlementStatus::Failed { .. } => {
                failed += 1;
            }
            SettlementStatus::Slashed { .. } => {
                failed += 1;
                slashing_events += 1;
            }
            _ => {}
        }
    }

    let total_settlements = successful + failed;
    let average_settlement_time = if completed_count > 0 {
        total_time / completed_count
    } else {
        0
    };

    // Create or update reputation
    let mut reputation =
        REPUTATIONS
            .may_load(deps.storage, &solver_id)?
            .unwrap_or(SolverReputation {
                solver_id: solver_id.clone(),
                total_settlements: 0,
                successful_settlements: 0,
                failed_settlements: 0,
                total_volume: Uint128::zero(),
                average_settlement_time: 0,
                slashing_events: 0,
                reputation_score: 5000, // Default starting score
                last_updated: env.block.time.seconds(),
            });

    reputation.total_settlements = total_settlements;
    reputation.successful_settlements = successful;
    reputation.failed_settlements = failed;
    reputation.total_volume = total_volume;
    reputation.average_settlement_time = average_settlement_time;
    reputation.slashing_events = slashing_events;
    reputation.last_updated = env.block.time.seconds();
    reputation.reputation_score = calculate_reputation_score(&reputation);

    REPUTATIONS.save(deps.storage, &solver_id, &reputation)?;

    Ok(Response::new()
        .add_attribute("action", "update_reputation")
        .add_attribute("solver_id", solver_id)
        .add_attribute("reputation_score", reputation.reputation_score.to_string()))
}

fn execute_decay_reputation(
    deps: DepsMut,
    env: Env,
    start_after: Option<String>,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    // Decay rate: 1% per day (86400 seconds)
    const DECAY_PERIOD: u64 = 86400; // 1 day in seconds
    const DECAY_BPS: u64 = 100; // 1% decay
    
    let limit = limit.unwrap_or(30).min(100) as usize;
    let start = start_after.as_ref().map(|s| cw_storage_plus::Bound::exclusive(s.as_str()));

    let mut updated_count = 0u32;
    let mut last_processed: Option<String> = None;

    // Iterate through reputations with pagination
    let all_reps: Vec<(String, SolverReputation)> = REPUTATIONS
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
        .take(limit)
        .filter_map(|r| r.ok())
        .collect();

    for (solver_id, mut rep) in all_reps {
        last_processed = Some(solver_id.clone());
        let time_since_update = env.block.time.seconds().saturating_sub(rep.last_updated);

        if time_since_update >= DECAY_PERIOD {
            let periods = time_since_update / DECAY_PERIOD;

            // Apply decay for each period
            for _ in 0..periods.min(30) {
                // Cap at 30 periods to avoid excessive computation per record
                let decay = (rep.reputation_score * DECAY_BPS) / 10000;
                rep.reputation_score = rep.reputation_score.saturating_sub(decay);
            }

            rep.last_updated = env.block.time.seconds();
            REPUTATIONS.save(deps.storage, &solver_id, &rep)?;
            updated_count += 1;
        }
    }

    let mut response = Response::new()
        .add_attribute("action", "decay_reputation")
        .add_attribute("updated_count", updated_count.to_string());
        
    if let Some(last_id) = last_processed {
        response = response.add_attribute("last_processed_solver", last_id);
    }

    Ok(response)
}

fn query_solver_reputation(deps: Deps, solver_id: String) -> StdResult<SolverReputationResponse> {
    let reputation = REPUTATIONS.load(deps.storage, &solver_id)?;
    Ok(reputation_to_response(reputation))
}

fn query_top_solvers(deps: Deps, limit: u32) -> StdResult<TopSolversResponse> {
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

fn query_solvers_by_reputation(
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

fn reputation_to_response(rep: SolverReputation) -> SolverReputationResponse {
    let fee_tier = match get_solver_fee_tier(rep.reputation_score) {
        FeeTier::Premium => "premium".to_string(),
        FeeTier::Standard => "standard".to_string(),
        FeeTier::Basic => "basic".to_string(),
        FeeTier::New => "new".to_string(),
    };

    SolverReputationResponse {
        solver_id: rep.solver_id,
        total_settlements: rep.total_settlements,
        successful_settlements: rep.successful_settlements,
        failed_settlements: rep.failed_settlements,
        total_volume: rep.total_volume,
        average_settlement_time: rep.average_settlement_time,
        slashing_events: rep.slashing_events,
        reputation_score: rep.reputation_score,
        fee_tier,
        last_updated: rep.last_updated,
    }
}

// ==================== ORIGINAL HELPER FUNCTIONS ====================

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

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
    use cosmwasm_std::{from_json, Addr, Coin, Timestamp};

    // Helper to get test addresses using MockApi
    struct TestAddrs {
        admin: Addr,
        escrow: Addr,
        solver_operator: Addr,
        user: Addr,
        random_user: Addr,
        new_admin: Addr,
        new_escrow: Addr,
    }

    fn test_addrs(api: &MockApi) -> TestAddrs {
        TestAddrs {
            admin: api.addr_make("admin"),
            escrow: api.addr_make("escrow"),
            solver_operator: api.addr_make("solver_operator"),
            user: api.addr_make("user"),
            random_user: api.addr_make("random_user"),
            new_admin: api.addr_make("new_admin"),
            new_escrow: api.addr_make("new_escrow"),
        }
    }

    fn setup_contract() -> (
        cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier,
        >,
        Env,
        TestAddrs,
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let addrs = test_addrs(&deps.api);

        let msg = InstantiateMsg {
            admin: addrs.admin.to_string(),
            escrow_contract: addrs.escrow.to_string(),
            min_solver_bond: Uint128::new(1_000_000),
            base_slash_bps: 200,
        };
        let info = message_info(&addrs.solver_operator, &[]);

        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        (deps, env, addrs)
    }

    fn register_solver(
        deps: &mut cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier,
        >,
        env: &Env,
        addrs: &TestAddrs,
        solver_id: &str,
        bond: u128,
    ) {
        let info = message_info(&addrs.solver_operator, &[Coin::new(bond, "uatom")]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::RegisterSolver {
                solver_id: solver_id.to_string(),
            },
        )
        .unwrap();
    }

    fn create_settlement(
        deps: &mut cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier,
        >,
        env: &Env,
        addrs: &TestAddrs,
        settlement_id: &str,
        solver_id: &str,
    ) {
        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::CreateSettlement {
                settlement_id: settlement_id.to_string(),
                intent_id: format!("intent_{}", settlement_id),
                solver_id: solver_id.to_string(),
                user: addrs.user.to_string(),
                user_input_amount: Uint128::new(100_000),
                user_input_denom: "uatom".to_string(),
                solver_output_amount: Uint128::new(1_000_000),
                solver_output_denom: "uusdc".to_string(),
                expires_at: env.block.time.seconds() + 3600, // 1 hour from now
            },
        )
        .unwrap();
    }

    // ==================== INSTANTIATION TESTS ====================

    #[test]
    fn test_instantiate_stores_config() {
        let (deps, _env, addrs) = setup_contract();

        let config: ConfigResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();

        assert_eq!(config.admin, addrs.admin.to_string());
        assert_eq!(config.escrow_contract, addrs.escrow.to_string());
        assert_eq!(config.min_solver_bond, Uint128::new(1_000_000));
        assert_eq!(config.base_slash_bps, 200);
    }

    // ==================== SOLVER REGISTRATION TESTS ====================

    #[test]
    fn test_register_solver_success() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.solver_operator, &[Coin::new(2_000_000u128, "uatom")]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::RegisterSolver {
                solver_id: "solver-1".to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.attributes.len(), 4);
        assert_eq!(res.attributes[0].value, "register_solver");

        let solver: SolverResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Solver {
                    solver_id: "solver-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(solver.id, "solver-1");
        assert_eq!(solver.operator, addrs.solver_operator.to_string());
        assert_eq!(solver.bond_amount, Uint128::new(2_000_000));
    }

    #[test]
    fn test_register_solver_insufficient_bond() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.solver_operator, &[Coin::new(500_000u128, "uatom")]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::RegisterSolver {
                solver_id: "solver-1".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InsufficientBond { .. }));
    }

    #[test]
    fn test_deregister_solver_returns_bond() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        let info = message_info(&addrs.solver_operator, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::DeregisterSolver {
                solver_id: "solver-1".to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes[0].value, "deregister_solver");
    }

    #[test]
    fn test_deregister_solver_unauthorized() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        let info = message_info(&addrs.random_user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::DeregisterSolver {
                solver_id: "solver-1".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    // ==================== SETTLEMENT CREATION TESTS ====================

    #[test]
    fn test_create_settlement_success() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        let info = message_info(&addrs.solver_operator, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::CreateSettlement {
                settlement_id: "settlement-1".to_string(),
                intent_id: "intent-1".to_string(),
                solver_id: "solver-1".to_string(),
                user: addrs.user.to_string(),
                user_input_amount: Uint128::new(100_000),
                user_input_denom: "uatom".to_string(),
                solver_output_amount: Uint128::new(1_000_000),
                solver_output_denom: "uusdc".to_string(),
                expires_at: env.block.time.seconds() + 3600,
            },
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "create_settlement");

        let settlement: SettlementResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Settlement {
                    settlement_id: "settlement-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(settlement.status, "pending");
    }

    #[test]
    fn test_create_settlement_solver_not_registered() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.admin, &[]);
        let err = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::CreateSettlement {
                settlement_id: "settlement-1".to_string(),
                intent_id: "intent-1".to_string(),
                solver_id: "nonexistent-solver".to_string(),
                user: addrs.user.to_string(),
                user_input_amount: Uint128::new(100_000),
                user_input_denom: "uatom".to_string(),
                solver_output_amount: Uint128::new(1_000_000),
                solver_output_denom: "uusdc".to_string(),
                expires_at: env.block.time.seconds() + 3600,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::SolverNotRegistered { .. }));
    }

    // ==================== STATE TRANSITION TESTS ====================

    #[test]
    fn test_mark_user_locked_by_escrow() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let settlement: SettlementResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Settlement {
                    settlement_id: "settlement-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(settlement.status, "user_locked");
    }

    #[test]
    fn test_mark_user_locked_unauthorized() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.random_user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn test_settlement_state_machine_happy_path() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Pending -> UserLocked
        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        // UserLocked -> SolverLocked
        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        // SolverLocked -> Executing
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkExecuting {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        // Executing -> Completed
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkCompleted {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let settlement: SettlementResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::Settlement {
                    settlement_id: "settlement-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(settlement.status, "completed");

        let solver: SolverResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Solver {
                    solver_id: "solver-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(solver.total_settlements, 1);
    }

    // ==================== EXECUTE SETTLEMENT TESTS ====================

    #[test]
    fn test_execute_settlement_success() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Move to SolverLocked state
        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        // Execute settlement
        let info = message_info(&addrs.admin, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "execute_settlement");

        let settlement: SettlementResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Settlement {
                    settlement_id: "settlement-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(settlement.status, "executing");
    }

    #[test]
    fn test_execute_settlement_wrong_state() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.admin, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidStateTransition { .. }));
    }

    #[test]
    fn test_execute_settlement_expired() {
        let (mut deps, mut env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        // Fast forward time past expiry
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

        let info = message_info(&addrs.admin, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::SettlementExpired {}));
    }

    // ==================== HANDLE TIMEOUT TESTS ====================

    #[test]
    fn test_handle_timeout_success() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Move to Executing state
        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap();

        // Handle timeout
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::HandleTimeout {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let settlement: SettlementResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Settlement {
                    settlement_id: "settlement-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(settlement.status, "failed: IBC transfer timeout");
    }

    #[test]
    fn test_handle_timeout_wrong_state() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.admin, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::HandleTimeout {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidStateTransition { .. }));
    }

    // ==================== HANDLE IBC ACK TESTS ====================

    #[test]
    fn test_handle_ibc_ack_success() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Move to Executing state
        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap();

        // Handle successful ack
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::HandleIbcAck {
                settlement_id: "settlement-1".to_string(),
                success: true,
            },
        )
        .unwrap();

        let settlement: SettlementResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Settlement {
                    settlement_id: "settlement-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(settlement.status, "completed");
    }

    #[test]
    fn test_handle_ibc_ack_failure() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Move to Executing state
        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap();

        // Handle failed ack
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::HandleIbcAck {
                settlement_id: "settlement-1".to_string(),
                success: false,
            },
        )
        .unwrap();

        let settlement: SettlementResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Settlement {
                    settlement_id: "settlement-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(settlement.status, "failed: IBC transfer failed");
    }

    // ==================== SLASH SOLVER TESTS ====================

    #[test]
    fn test_slash_solver_success() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.admin, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::SlashSolver {
                solver_id: "solver-1".to_string(),
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "slash_solver");

        let solver: SolverResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Solver {
                    solver_id: "solver-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        // 2_000_000 - 2000 = 1_998_000 (2% of 100_000 = 2000)
        assert_eq!(solver.bond_amount, Uint128::new(1_998_000));
    }

    #[test]
    fn test_slash_solver_unauthorized() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.random_user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::SlashSolver {
                solver_id: "solver-1".to_string(),
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    // ==================== UPDATE CONFIG TESTS ====================

    #[test]
    fn test_update_config_success() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateConfig {
                admin: Some(addrs.new_admin.to_string()),
                escrow_contract: Some(addrs.new_escrow.to_string()),
                min_solver_bond: Some(Uint128::new(5_000_000)),
                base_slash_bps: Some(500),
            },
        )
        .unwrap();

        let config: ConfigResponse =
            from_json(query(deps.as_ref(), env, QueryMsg::Config {}).unwrap()).unwrap();

        assert_eq!(config.admin, addrs.new_admin.to_string());
        assert_eq!(config.min_solver_bond, Uint128::new(5_000_000));
    }

    #[test]
    fn test_update_config_unauthorized() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.random_user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::UpdateConfig {
                admin: Some(addrs.new_admin.to_string()),
                escrow_contract: None,
                min_solver_bond: None,
                base_slash_bps: None,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    // ==================== QUERY TESTS ====================

    #[test]
    fn test_query_solvers() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        register_solver(&mut deps, &env, &addrs, "solver-2", 3_000_000);

        let response: SolversResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Solvers {
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(response.solvers.len(), 2);
    }

    #[test]
    fn test_query_settlement_not_found() {
        let (deps, env, _addrs) = setup_contract();

        let err = query(
            deps.as_ref(),
            env,
            QueryMsg::Settlement {
                settlement_id: "nonexistent".to_string(),
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("not found"));
    }

    // ==================== REPUTATION SYSTEM TESTS ====================

    #[test]
    fn test_update_reputation_new_solver() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        let info = message_info(&addrs.admin, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateReputation {
                solver_id: "solver-1".to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "update_reputation");

        let reputation: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::SolverReputation {
                    solver_id: "solver-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        // New solver with no settlements should have default score
        assert_eq!(reputation.reputation_score, 5000);
        assert_eq!(reputation.total_settlements, 0);
        assert_eq!(reputation.successful_settlements, 0);
        assert_eq!(reputation.failed_settlements, 0);
    }

    #[test]
    fn test_update_reputation_with_settlements() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        // Create and complete a settlement
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkExecuting {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkCompleted {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        // Update reputation
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateReputation {
                solver_id: "solver-1".to_string(),
            },
        )
        .unwrap();

        let reputation: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::SolverReputation {
                    solver_id: "solver-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        // Should have 1 successful settlement
        assert_eq!(reputation.total_settlements, 1);
        assert_eq!(reputation.successful_settlements, 1);
        assert_eq!(reputation.failed_settlements, 0);
        assert!(reputation.reputation_score > 5000); // Should be higher than default
    }

    #[test]
    fn test_reputation_score_calculation() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        // Create multiple settlements - some successful, some failed
        for i in 0..10 {
            create_settlement(
                &mut deps,
                &env,
                &addrs,
                &format!("settlement-{}", i),
                "solver-1",
            );

            let info = message_info(&addrs.escrow, &[]);
            execute(
                deps.as_mut(),
                env.clone(),
                info,
                ExecuteMsg::MarkUserLocked {
                    settlement_id: format!("settlement-{}", i),
                    escrow_id: format!("escrow-{}", i),
                },
            )
            .unwrap();

            let info = message_info(&addrs.solver_operator, &[]);
            execute(
                deps.as_mut(),
                env.clone(),
                info,
                ExecuteMsg::MarkSolverLocked {
                    settlement_id: format!("settlement-{}", i),
                },
            )
            .unwrap();

            let info = message_info(&addrs.admin, &[]);
            execute(
                deps.as_mut(),
                env.clone(),
                info,
                ExecuteMsg::MarkExecuting {
                    settlement_id: format!("settlement-{}", i),
                },
            )
            .unwrap();

            // Complete 8 successfully, fail 2
            if i < 8 {
                let info = message_info(&addrs.admin, &[]);
                execute(
                    deps.as_mut(),
                    env.clone(),
                    info,
                    ExecuteMsg::MarkCompleted {
                        settlement_id: format!("settlement-{}", i),
                    },
                )
                .unwrap();
            } else {
                let info = message_info(&addrs.admin, &[]);
                execute(
                    deps.as_mut(),
                    env.clone(),
                    info,
                    ExecuteMsg::MarkFailed {
                        settlement_id: format!("settlement-{}", i),
                        reason: "test failure".to_string(),
                    },
                )
                .unwrap();
            }
        }

        // Update reputation
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateReputation {
                solver_id: "solver-1".to_string(),
            },
        )
        .unwrap();

        let reputation: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::SolverReputation {
                    solver_id: "solver-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        // 80% success rate should result in good score
        assert_eq!(reputation.total_settlements, 10);
        assert_eq!(reputation.successful_settlements, 8);
        assert_eq!(reputation.failed_settlements, 2);
        assert!(reputation.reputation_score > 5000);
    }

    #[test]
    fn test_reputation_fee_tiers() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-premium", 2_000_000);
        register_solver(&mut deps, &env, &addrs, "solver-standard", 2_000_000);
        register_solver(&mut deps, &env, &addrs, "solver-basic", 2_000_000);
        register_solver(&mut deps, &env, &addrs, "solver-new", 2_000_000);

        // Create reputations with different scores
        let premium_rep = SolverReputation {
            solver_id: "solver-premium".to_string(),
            total_settlements: 100,
            successful_settlements: 95,
            failed_settlements: 5,
            total_volume: Uint128::new(10_000_000),
            average_settlement_time: 30,
            slashing_events: 0,
            reputation_score: 9500,
            last_updated: env.block.time.seconds(),
        };

        let standard_rep = SolverReputation {
            solver_id: "solver-standard".to_string(),
            total_settlements: 50,
            successful_settlements: 45,
            failed_settlements: 5,
            total_volume: Uint128::new(5_000_000),
            average_settlement_time: 90,
            slashing_events: 0,
            reputation_score: 7500,
            last_updated: env.block.time.seconds(),
        };

        let basic_rep = SolverReputation {
            solver_id: "solver-basic".to_string(),
            total_settlements: 20,
            successful_settlements: 15,
            failed_settlements: 5,
            total_volume: Uint128::new(1_000_000),
            average_settlement_time: 150,
            slashing_events: 1,
            reputation_score: 5500,
            last_updated: env.block.time.seconds(),
        };

        let new_rep = SolverReputation {
            solver_id: "solver-new".to_string(),
            total_settlements: 5,
            successful_settlements: 3,
            failed_settlements: 2,
            total_volume: Uint128::new(100_000),
            average_settlement_time: 200,
            slashing_events: 0,
            reputation_score: 4000,
            last_updated: env.block.time.seconds(),
        };

        REPUTATIONS
            .save(deps.as_mut().storage, "solver-premium", &premium_rep)
            .unwrap();
        REPUTATIONS
            .save(deps.as_mut().storage, "solver-standard", &standard_rep)
            .unwrap();
        REPUTATIONS
            .save(deps.as_mut().storage, "solver-basic", &basic_rep)
            .unwrap();
        REPUTATIONS
            .save(deps.as_mut().storage, "solver-new", &new_rep)
            .unwrap();

        // Query and check fee tiers
        let premium: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::SolverReputation {
                    solver_id: "solver-premium".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(premium.fee_tier, "premium");

        let standard: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::SolverReputation {
                    solver_id: "solver-standard".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(standard.fee_tier, "standard");

        let basic: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::SolverReputation {
                    solver_id: "solver-basic".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(basic.fee_tier, "basic");

        let new: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::SolverReputation {
                    solver_id: "solver-new".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(new.fee_tier, "new");
    }

    #[test]
    fn test_top_solvers_query() {
        let (mut deps, env, addrs) = setup_contract();

        // Create multiple solvers with different reputations
        for i in 0..5 {
            register_solver(&mut deps, &env, &addrs, &format!("solver-{}", i), 2_000_000);

            let rep = SolverReputation {
                solver_id: format!("solver-{}", i),
                total_settlements: 10 * (i + 1),
                successful_settlements: 9 * (i + 1),
                failed_settlements: i + 1,
                total_volume: Uint128::new(1_000_000 * (i as u128 + 1)),
                average_settlement_time: 60,
                slashing_events: 0,
                reputation_score: 5000 + (1000 * i),
                last_updated: env.block.time.seconds(),
            };

            REPUTATIONS
                .save(deps.as_mut().storage, &format!("solver-{}", i), &rep)
                .unwrap();
        }

        // Query top 3 solvers
        let response: TopSolversResponse =
            from_json(query(deps.as_ref(), env, QueryMsg::TopSolvers { limit: 3 }).unwrap())
                .unwrap();

        assert_eq!(response.solvers.len(), 3);
        // Should be sorted by reputation score (descending)
        assert_eq!(response.solvers[0].solver_id, "solver-4");
        assert_eq!(response.solvers[1].solver_id, "solver-3");
        assert_eq!(response.solvers[2].solver_id, "solver-2");
    }

    #[test]
    fn test_solvers_by_reputation_query() {
        let (mut deps, env, addrs) = setup_contract();

        // Create solvers with various reputation scores
        for i in 0..5 {
            register_solver(&mut deps, &env, &addrs, &format!("solver-{}", i), 2_000_000);

            let rep = SolverReputation {
                solver_id: format!("solver-{}", i),
                total_settlements: 10,
                successful_settlements: 9,
                failed_settlements: 1,
                total_volume: Uint128::new(1_000_000),
                average_settlement_time: 60,
                slashing_events: 0,
                reputation_score: 5000 + (1000 * i),
                last_updated: env.block.time.seconds(),
            };

            REPUTATIONS
                .save(deps.as_mut().storage, &format!("solver-{}", i), &rep)
                .unwrap();
        }

        // Query solvers with min_score of 7000
        let response: SolversByReputationResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::SolversByReputation {
                    min_score: 7000,
                    limit: 10,
                },
            )
            .unwrap(),
        )
        .unwrap();

        // Should only return solvers with score >= 7000
        assert_eq!(response.solvers.len(), 3); // solver-2, solver-3, solver-4
        for solver in &response.solvers {
            assert!(solver.reputation_score >= 7000);
        }
    }

    #[test]
    fn test_reputation_decay() {
        let (mut deps, mut env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        // Manually set initial reputation
        let initial_rep = SolverReputation {
            solver_id: "solver-1".to_string(),
            total_settlements: 10,
            successful_settlements: 9,
            failed_settlements: 1,
            total_volume: Uint128::new(1_000_000),
            average_settlement_time: 60,
            slashing_events: 0,
            reputation_score: 8000,
            last_updated: env.block.time.seconds(),
        };

        REPUTATIONS
            .save(deps.as_mut().storage, "solver-1", &initial_rep)
            .unwrap();

        // Fast forward 5 days (5 * 86400 seconds)
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 5 * 86400);

        // Execute decay
        let info = message_info(&addrs.admin, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::DecayReputation {
                start_after: None,
                limit: None,
            },
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "decay_reputation");

        let reputation: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::SolverReputation {
                    solver_id: "solver-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        // After 5 days of 1% decay per day, score should be lower
        assert!(reputation.reputation_score < 8000);
        assert!(reputation.reputation_score > 7000); // But not too much lower
    }

    #[test]
    fn test_reputation_decay_pagination() {
        let (mut deps, mut env, addrs) = setup_contract();
        
        // Register multiple solvers
        for i in 1..=5 {
            let id = format!("solver-{}", i);
            register_solver(&mut deps, &env, &addrs, &id, 2_000_000);
            
            let rep = SolverReputation {
                solver_id: id.clone(),
                total_settlements: 10,
                successful_settlements: 10,
                failed_settlements: 0,
                total_volume: Uint128::new(1_000_000),
                average_settlement_time: 60,
                slashing_events: 0,
                reputation_score: 10000,
                last_updated: env.block.time.seconds(),
            };
            REPUTATIONS.save(deps.as_mut().storage, &id, &rep).unwrap();
        }

        // Fast forward 5 days
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 5 * 86400);

        // First page: limit 2
        let info = message_info(&addrs.admin, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ExecuteMsg::DecayReputation {
                start_after: None,
                limit: Some(2),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[1].key, "updated_count");
        assert_eq!(res.attributes[1].value, "2");
        assert_eq!(res.attributes[2].key, "last_processed_solver");
        
        let last_processed = res.attributes[2].value.clone();

        // Second page: limit 2, start after last
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ExecuteMsg::DecayReputation {
                start_after: Some(last_processed.clone()),
                limit: Some(2),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[1].value, "2");
        let next_last = res.attributes[2].value.clone();
        assert_ne!(last_processed, next_last);

        // Third page: remaining 1
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::DecayReputation {
                start_after: Some(next_last),
                limit: Some(2),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[1].value, "1");
    }

    #[test]
    fn test_reputation_with_slashing() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        // Create settlement and slash solver
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::SlashSolver {
                solver_id: "solver-1".to_string(),
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        // Update reputation
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateReputation {
                solver_id: "solver-1".to_string(),
            },
        )
        .unwrap();

        let reputation: SolverReputationResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::SolverReputation {
                    solver_id: "solver-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        // Slashing should reduce reputation score
        assert_eq!(reputation.slashing_events, 1);
        assert!(reputation.reputation_score < 5000); // Should be below default
    }

    #[test]
    fn test_reputation_not_found() {
        let (deps, env, _addrs) = setup_contract();

        let err = query(
            deps.as_ref(),
            env,
            QueryMsg::SolverReputation {
                solver_id: "nonexistent".to_string(),
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("not found"));
    }

    // ==================== ESCROW RELEASE/REFUND TESTS ====================

    #[test]
    fn test_handle_ibc_ack_success_generates_release_message() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Move to Executing state
        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap();

        // Handle successful ack
        let info = message_info(&addrs.admin, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::HandleIbcAck {
                settlement_id: "settlement-1".to_string(),
                success: true,
            },
        )
        .unwrap();

        // Verify we have a message
        assert_eq!(res.messages.len(), 1);

        // Verify attributes
        assert_eq!(res.attributes[0].value, "handle_ibc_ack");
        assert_eq!(res.attributes[2].value, "success");
        assert_eq!(res.attributes[3].key, "escrow_id");
        assert_eq!(res.attributes[3].value, "escrow-123");
        assert_eq!(res.attributes[4].key, "release_to_solver");
        assert_eq!(res.attributes[4].value, addrs.solver_operator.to_string());
    }

    #[test]
    fn test_handle_ibc_ack_failure_generates_refund_message() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Move to Executing state
        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-123".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap();

        // Handle failed ack
        let info = message_info(&addrs.admin, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::HandleIbcAck {
                settlement_id: "settlement-1".to_string(),
                success: false,
            },
        )
        .unwrap();

        // Verify we have a message
        assert_eq!(res.messages.len(), 1);

        // Verify attributes
        assert_eq!(res.attributes[0].value, "handle_ibc_ack");
        assert_eq!(res.attributes[2].value, "failure");
        assert_eq!(res.attributes[3].key, "escrow_id");
        assert_eq!(res.attributes[3].value, "escrow-123");
    }

    #[test]
    fn test_handle_timeout_generates_refund_message() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Move to Executing state
        let info = message_info(&addrs.escrow, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkUserLocked {
                settlement_id: "settlement-1".to_string(),
                escrow_id: "escrow-456".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::MarkSolverLocked {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::ExecuteSettlement {
                settlement_id: "settlement-1".to_string(),
                ibc_channel: "channel-0".to_string(),
            },
        )
        .unwrap();

        // Handle timeout
        let info = message_info(&addrs.admin, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::HandleTimeout {
                settlement_id: "settlement-1".to_string(),
            },
        )
        .unwrap();

        // Verify we have a message
        assert_eq!(res.messages.len(), 1);

        // Verify attributes
        assert_eq!(res.attributes[0].value, "handle_timeout");
        assert_eq!(res.attributes[2].key, "escrow_id");
        assert_eq!(res.attributes[2].value, "escrow-456");
    }

    #[test]
    fn test_handle_ibc_ack_without_escrow_id_fails() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
        create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

        // Move to Executing state without setting escrow_id
        let mut settlement = SETTLEMENTS.load(&deps.storage, "settlement-1").unwrap();
        settlement.status = SettlementStatus::Executing;
        SETTLEMENTS
            .save(&mut deps.storage, "settlement-1", &settlement)
            .unwrap();

        // Try to handle ack - should fail because escrow_id is None
        let info = message_info(&addrs.admin, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::HandleIbcAck {
                settlement_id: "settlement-1".to_string(),
                success: true,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidStateTransition { .. }));
    }

    #[test]
    fn test_create_settlement_unauthorized_access() {
        let (mut deps, env, addrs) = setup_contract();
        register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

        // Attacker tries to create a settlement on behalf of solver-1
        let info = message_info(&addrs.random_user, &[]);
        
        // This should now FAIL with Unauthorized
        let err = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::CreateSettlement {
                settlement_id: "fake-settlement".to_string(),
                intent_id: "intent-123".to_string(),
                solver_id: "solver-1".to_string(),
                user: addrs.user.to_string(),
                user_input_amount: Uint128::new(100),
                user_input_denom: "uatom".to_string(),
                solver_output_amount: Uint128::new(100),
                solver_output_denom: "uusdc".to_string(),
                expires_at: env.block.time.seconds() + 3600,
            },
        ).unwrap_err();

        // Assert that it was rejected
        assert!(matches!(err, ContractError::Unauthorized {}));
    }
}
