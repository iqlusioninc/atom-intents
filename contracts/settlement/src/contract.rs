use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdResult,
};

use crate::error::ContractError;
use crate::handlers::{
    execute_create_settlement, execute_decay_reputation, execute_deregister_solver,
    execute_handle_ibc_ack, execute_handle_timeout, execute_mark_completed, execute_mark_executing,
    execute_mark_failed, execute_mark_solver_locked, execute_mark_user_locked,
    execute_register_solver, execute_settlement, execute_slash_solver, execute_update_config,
    execute_update_reputation,
};
use crate::msg::{
    ConfigUpdate, ExecuteMsg, InflightSettlementsResponse, InstantiateMsg, MigrateMsg,
    MigrationInfoResponse, QueryMsg, StuckSettlementAction,
};
use crate::queries::{
    query_config, query_settlement, query_settlement_by_intent, query_settlements_by_solver,
    query_solver, query_solver_reputation, query_solvers, query_solvers_by_reputation,
    query_top_solvers,
};
use crate::state::{
    Config, MigrationInfo, SettlementStatus, CONFIG, MIGRATION_INFO, SETTLEMENTS,
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
        QueryMsg::MigrationInfo {} => to_json_binary(&query_migration_info(deps)?),
        QueryMsg::InflightSettlements { start_after, limit } => {
            to_json_binary(&query_inflight_settlements(deps, start_after, limit)?)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MIGRATION ENTRY POINT
// ═══════════════════════════════════════════════════════════════════════════

/// Contract version for tracking migrations
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CONTRACT_NAME: &str = "crates.io:atom-intents-settlement";

#[entry_point]
pub fn migrate(deps: DepsMut, env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    // 1. Get current migration info (if exists)
    let current_info = MIGRATION_INFO.may_load(deps.storage)?;
    let previous_version = current_info.map(|i| i.current_version);

    // 2. Count inflight settlements
    let inflight = get_inflight_settlement_ids(deps.as_ref())?;
    let inflight_count = inflight.len() as u64;

    // 3. Check if migration is allowed with inflight settlements
    if inflight_count > 0 {
        let preserve = msg.config.as_ref().map(|c| c.preserve_inflight).unwrap_or(true);
        if !preserve {
            return Err(ContractError::InflightSettlementsExist {
                count: inflight_count,
            });
        }
    }

    // 4. Handle stuck settlements based on configuration
    if let Some(ref config) = msg.config {
        handle_stuck_settlements(deps.storage, &env, &config.stuck_settlement_action, &inflight)?;

        // Extend timeouts if requested
        if let Some(extend_secs) = config.extend_timeout_secs {
            extend_inflight_timeouts(deps.storage, extend_secs, &inflight)?;
        }
    }

    // 5. Apply new configuration if provided
    if let Some(ref config) = msg.config {
        if let Some(ref new_config) = config.new_config {
            apply_config_update(deps.storage, deps.api, new_config)?;
        }
    }

    // 6. Save migration info
    let migration_info = MigrationInfo {
        previous_version: previous_version.clone(),
        current_version: msg.new_version.clone(),
        migrated_at: Some(env.block.time.seconds()),
        preserved_inflight_count: inflight_count,
    };
    MIGRATION_INFO.save(deps.storage, &migration_info)?;

    // 7. Emit migration event
    Ok(Response::new()
        .add_attribute("action", "migrate")
        .add_attribute(
            "from_version",
            previous_version.unwrap_or_else(|| "none".to_string()),
        )
        .add_attribute("to_version", msg.new_version)
        .add_attribute("preserved_inflight", inflight_count.to_string()))
}

// ═══════════════════════════════════════════════════════════════════════════
// MIGRATION HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════

/// Get all settlement IDs that are not in a terminal state
fn get_inflight_settlement_ids(deps: Deps) -> StdResult<Vec<String>> {
    let settlements: Vec<_> = SETTLEMENTS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|r| {
            r.ok().and_then(|(id, settlement)| {
                if !is_terminal_status(&settlement.status) {
                    Some(id)
                } else {
                    None
                }
            })
        })
        .collect();
    Ok(settlements)
}

/// Check if a settlement status is terminal (completed, failed, slashed)
fn is_terminal_status(status: &SettlementStatus) -> bool {
    matches!(
        status,
        SettlementStatus::Completed | SettlementStatus::Failed { .. } | SettlementStatus::Slashed { .. }
    )
}

/// Handle stuck settlements based on the configured action
fn handle_stuck_settlements(
    storage: &mut dyn cosmwasm_std::Storage,
    env: &Env,
    action: &StuckSettlementAction,
    inflight_ids: &[String],
) -> Result<(), ContractError> {
    let current_time = env.block.time.seconds();

    for id in inflight_ids {
        if let Some(mut settlement) = SETTLEMENTS.may_load(storage, id)? {
            // Check if settlement is stuck (past expiry)
            if settlement.expires_at < current_time {
                match action {
                    StuckSettlementAction::Preserve => {
                        // Do nothing, keep as-is
                    }
                    StuckSettlementAction::RefundAndFail => {
                        settlement.status = SettlementStatus::Failed {
                            reason: "Marked failed during migration (past expiry)".to_string(),
                        };
                        SETTLEMENTS.save(storage, id, &settlement)?;
                    }
                    StuckSettlementAction::ExtendTimeout { additional_seconds } => {
                        settlement.expires_at = current_time + additional_seconds;
                        SETTLEMENTS.save(storage, id, &settlement)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Extend timeouts for all inflight settlements
fn extend_inflight_timeouts(
    storage: &mut dyn cosmwasm_std::Storage,
    extend_secs: u64,
    inflight_ids: &[String],
) -> Result<(), ContractError> {
    for id in inflight_ids {
        if let Some(mut settlement) = SETTLEMENTS.may_load(storage, id)? {
            settlement.expires_at += extend_secs;
            SETTLEMENTS.save(storage, id, &settlement)?;
        }
    }
    Ok(())
}

/// Apply configuration updates during migration
fn apply_config_update<A: cosmwasm_std::Api + ?Sized>(
    storage: &mut dyn cosmwasm_std::Storage,
    api: &A,
    update: &ConfigUpdate,
) -> Result<(), ContractError> {
    let mut config = CONFIG.load(storage)?;

    if let Some(ref admin) = update.admin {
        config.admin = api.addr_validate(admin)?;
    }
    if let Some(ref escrow) = update.escrow_contract {
        config.escrow_contract = api.addr_validate(escrow)?;
    }
    if let Some(bond) = update.min_solver_bond {
        config.min_solver_bond = bond;
    }
    if let Some(slash_bps) = update.base_slash_bps {
        config.base_slash_bps = slash_bps;
    }

    CONFIG.save(storage, &config)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// MIGRATION QUERY HANDLERS
// ═══════════════════════════════════════════════════════════════════════════

/// Query migration information
fn query_migration_info(deps: Deps) -> StdResult<MigrationInfoResponse> {
    let info = MIGRATION_INFO.may_load(deps.storage)?;

    match info {
        Some(info) => Ok(MigrationInfoResponse {
            previous_version: info.previous_version,
            current_version: info.current_version,
            migrated_at: info.migrated_at,
            preserved_inflight_count: info.preserved_inflight_count,
        }),
        None => Ok(MigrationInfoResponse {
            previous_version: None,
            current_version: CONTRACT_VERSION.to_string(),
            migrated_at: None,
            preserved_inflight_count: 0,
        }),
    }
}

/// Query inflight settlements (not in terminal state)
fn query_inflight_settlements(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<InflightSettlementsResponse> {
    let limit = limit.unwrap_or(100).min(500) as usize;
    let start = start_after.as_deref().map(cw_storage_plus::Bound::exclusive);

    let settlement_ids: Vec<String> = SETTLEMENTS
        .range(deps.storage, start, None, Order::Ascending)
        .filter_map(|r| {
            r.ok().and_then(|(id, settlement)| {
                if !is_terminal_status(&settlement.status) {
                    Some(id)
                } else {
                    None
                }
            })
        })
        .take(limit)
        .collect();

    let count = settlement_ids.len() as u64;

    Ok(InflightSettlementsResponse {
        settlement_ids,
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
    use cosmwasm_std::{from_json, Addr, Coin, Timestamp, Uint128};

    use crate::msg::{
        ConfigResponse, SettlementResponse, SolverReputationResponse, SolverResponse,
        SolversResponse, TopSolversResponse,
    };
    use crate::state::{SolverReputation, SettlementStatus, REPUTATIONS, SETTLEMENTS, SOLVERS};

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
        // Use 100 ATOM bond (100_000_000 uatom) to absorb MIN_SLASH_AMOUNT (10 ATOM)
        register_solver(&mut deps, &env, &addrs, "solver-1", 100_000_000);
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
        // Calculated: 2% of 100_000 = 2000, but MIN_SLASH_AMOUNT = 10_000_000 (10 ATOM)
        // Actual slash: max(2000, 10_000_000) = 10_000_000
        // Remaining bond: 100_000_000 - 10_000_000 = 90_000_000
        assert_eq!(solver.bond_amount, Uint128::new(90_000_000));
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
        use crate::msg::SolversByReputationResponse;

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
        // Use 100 ATOM bond to absorb MIN_SLASH_AMOUNT (10 ATOM)
        register_solver(&mut deps, &env, &addrs, "solver-1", 100_000_000);

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
        )
        .unwrap_err();

        // Assert that it was rejected
        assert!(matches!(err, ContractError::Unauthorized {}));
    }
}
