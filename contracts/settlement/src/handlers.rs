use cosmwasm_std::{
    to_json_binary, BankMsg, Coin, DepsMut, Env, IbcMsg, IbcTimeout, MessageInfo, Response,
    Uint128, WasmMsg,
};

use crate::error::ContractError;
use crate::state::{
    RegisteredSolver, Settlement, SettlementStatus, SolverReputation, CONFIG,
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

pub fn execute_register_solver(
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

pub fn execute_deregister_solver(
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

pub fn execute_create_settlement(
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
    let solver =
        SOLVERS
            .load(deps.storage, &solver_id)
            .map_err(|_| ContractError::SolverNotRegistered {
                id: solver_id.clone(),
            })?;

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

pub fn execute_mark_user_locked(
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

    // SECURITY FIX (5.6/7.1): Validate state transition
    let target_status = SettlementStatus::UserLocked;
    if !settlement.status.can_transition_to(&target_status) {
        return Err(ContractError::InvalidStateTransition {
            from: settlement.status.as_str().to_string(),
            to: target_status.as_str().to_string(),
        });
    }

    settlement.status = target_status;
    settlement.escrow_id = Some(escrow_id);
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_user_locked")
        .add_attribute("settlement_id", settlement_id))
}

pub fn execute_mark_solver_locked(
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

    // SECURITY FIX (5.6/7.1): Validate state transition
    let target_status = SettlementStatus::SolverLocked;
    if !settlement.status.can_transition_to(&target_status) {
        return Err(ContractError::InvalidStateTransition {
            from: settlement.status.as_str().to_string(),
            to: target_status.as_str().to_string(),
        });
    }

    settlement.status = target_status;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_solver_locked")
        .add_attribute("settlement_id", settlement_id))
}

pub fn execute_mark_executing(
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

    // SECURITY FIX (5.6/7.1): Validate state transition
    let target_status = SettlementStatus::Executing;
    if !settlement.status.can_transition_to(&target_status) {
        return Err(ContractError::InvalidStateTransition {
            from: settlement.status.as_str().to_string(),
            to: target_status.as_str().to_string(),
        });
    }

    settlement.status = target_status;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_executing")
        .add_attribute("settlement_id", settlement_id))
}

pub fn execute_mark_completed(
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

    // SECURITY FIX (7.1): Validate state transition - prevents double completion
    let target_status = SettlementStatus::Completed;
    if !settlement.status.can_transition_to(&target_status) {
        return Err(ContractError::InvalidStateTransition {
            from: settlement.status.as_str().to_string(),
            to: target_status.as_str().to_string(),
        });
    }

    // Update solver stats
    let mut solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
    solver.total_settlements += 1;
    SOLVERS.save(deps.storage, &settlement.solver_id, &solver)?;

    settlement.status = target_status;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_completed")
        .add_attribute("settlement_id", settlement_id))
}

pub fn execute_mark_failed(
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

    // SECURITY FIX (5.6/7.1): Validate state transition
    let target_status = SettlementStatus::Failed {
        reason: reason.clone(),
    };
    if !settlement.status.can_transition_to(&target_status) {
        return Err(ContractError::InvalidStateTransition {
            from: settlement.status.as_str().to_string(),
            to: "Failed".to_string(),
        });
    }

    // Update solver stats
    let mut solver = SOLVERS.load(deps.storage, &settlement.solver_id)?;
    solver.total_settlements += 1;
    solver.failed_settlements += 1;
    SOLVERS.save(deps.storage, &settlement.solver_id, &solver)?;

    settlement.status = target_status;
    SETTLEMENTS.save(deps.storage, &settlement_id, &settlement)?;

    Ok(Response::new()
        .add_attribute("action", "mark_failed")
        .add_attribute("settlement_id", settlement_id)
        .add_attribute("reason", reason))
}

pub fn execute_slash_solver(
    deps: DepsMut,
    info: MessageInfo,
    solver_id: String,
    settlement_id: String,
) -> Result<Response, ContractError> {
    use crate::state::MIN_SLASH_AMOUNT;

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

    let mut settlement = SETTLEMENTS
        .load(deps.storage, &settlement_id)
        .map_err(|_| ContractError::SettlementNotFound {
            id: settlement_id.clone(),
        })?;

    // SECURITY FIX (5.6/7.1): Validate state transition
    let target_status = SettlementStatus::Slashed {
        amount: Uint128::zero(), // Placeholder, actual amount calculated below
    };
    if !settlement.status.can_transition_to(&target_status) {
        return Err(ContractError::InvalidStateTransition {
            from: settlement.status.as_str().to_string(),
            to: "Slashed".to_string(),
        });
    }

    // Calculate slash amount (base_slash_bps of settlement value)
    let calculated_slash = settlement.user_input_amount * Uint128::from(config.base_slash_bps)
        / Uint128::from(10000u64);

    // SECURITY FIX (1.7): Apply minimum slash threshold to prevent dust attacks
    let slash_with_minimum = std::cmp::max(calculated_slash, Uint128::new(MIN_SLASH_AMOUNT));

    // Cap at solver's bond amount
    let actual_slash = std::cmp::min(slash_with_minimum, solver.bond_amount);

    solver.bond_amount = solver.bond_amount.saturating_sub(actual_slash);
    SOLVERS.save(deps.storage, &solver_id, &solver)?;

    // Update settlement status
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

pub fn execute_update_config(
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

pub fn execute_settlement(
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

pub fn execute_handle_timeout(
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

pub fn execute_handle_ibc_ack(
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

pub fn execute_update_reputation(
    deps: DepsMut,
    env: Env,
    solver_id: String,
) -> Result<Response, ContractError> {
    use crate::helpers::calculate_reputation_score;

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

pub fn execute_decay_reputation(
    deps: DepsMut,
    env: Env,
    start_after: Option<String>,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    // Decay rate: 1% per day (86400 seconds)
    const DECAY_PERIOD: u64 = 86400; // 1 day in seconds
    const DECAY_BPS: u64 = 100; // 1% decay

    let limit = limit.unwrap_or(30).min(100) as usize;
    let start = start_after
        .as_ref()
        .map(|s| cw_storage_plus::Bound::exclusive(s.as_str()));

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
