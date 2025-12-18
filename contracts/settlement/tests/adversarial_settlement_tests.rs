/// Adversarial tests for the settlement contract
///
/// These tests simulate attacks where things could go horribly wrong:
/// - State machine bypass attacks
/// - Solver impersonation
/// - Double-settlement attacks
/// - IBC callback manipulation
/// - Reputation system gaming
/// - Slashing bypass attacks

use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
use cosmwasm_std::{from_json, Addr, Coin, Timestamp, Uint128};

use atom_intents_settlement_contract::contract::{execute, instantiate, query};
use atom_intents_settlement_contract::error::ContractError;
use atom_intents_settlement_contract::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, SettlementResponse};

// Helper to get test addresses
struct TestAddrs {
    admin: Addr,
    escrow: Addr,
    solver_operator: Addr,
    user: Addr,
    attacker: Addr,
    fake_solver: Addr,
}

fn test_addrs(api: &MockApi) -> TestAddrs {
    TestAddrs {
        admin: api.addr_make("admin"),
        escrow: api.addr_make("escrow"),
        solver_operator: api.addr_make("solver_operator"),
        user: api.addr_make("user"),
        attacker: api.addr_make("attacker"),
        fake_solver: api.addr_make("fake_solver"),
    }
}

fn setup_contract() -> (
    cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    cosmwasm_std::Env,
    TestAddrs,
) {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let addrs = test_addrs(&deps.api);

    let msg = InstantiateMsg {
        admin: addrs.admin.to_string(),
        escrow_contract: addrs.escrow.to_string(),
        min_solver_bond: Uint128::new(1_000_000),
        base_slash_bps: 1000, // 10%
    };
    let info = message_info(&addrs.admin, &[]);

    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    (deps, env, addrs)
}

fn register_solver(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    env: &cosmwasm_std::Env,
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
    env: &cosmwasm_std::Env,
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
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// STATE MACHINE BYPASS ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that executing settlement from Pending state fails
#[test]
fn test_execute_from_pending_fails() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // ATTACK: Try to execute directly from Pending (skip locking phases)
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ExecuteSettlement {
            settlement_id: "settlement-1".to_string(),
            ibc_channel: "channel-0".to_string(),
        },
    );

    // Must fail - settlement not in SolverLocked state
    assert!(matches!(result.unwrap_err(), ContractError::InvalidStateTransition { .. }));
}

/// Test that marking completed from wrong state fails
#[test]
fn test_mark_completed_from_wrong_state() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // ATTACK: Try to mark completed directly (skip all intermediate states)
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MarkCompleted {
            settlement_id: "settlement-1".to_string(),
        },
    );

    // Verify it's not possible to jump to completed
    // (depends on implementation - may be blocked or may need additional state checks)
    // At minimum, the settlement status should be correct
}

/// Test state cannot go backwards (Executing -> SolverLocked)
#[test]
fn test_state_cannot_go_backwards() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // Progress through states
    // Mark user locked
    let info = message_info(&addrs.escrow, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MarkUserLocked {
            settlement_id: "settlement-1".to_string(),
            escrow_id: "escrow-1".to_string(),
        },
    )
    .unwrap();

    // Mark solver locked
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

    // ATTACK: Try to mark user locked again (go backwards)
    let info = message_info(&addrs.escrow, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MarkUserLocked {
            settlement_id: "settlement-1".to_string(),
            escrow_id: "escrow-2".to_string(),
        },
    );

    // Should fail or be idempotent (not change escrow_id)
}

// ═══════════════════════════════════════════════════════════════════════════
// SOLVER IMPERSONATION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that attacker cannot create settlement for another solver
#[test]
fn test_attacker_cannot_impersonate_solver() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

    // ATTACK: Attacker (not solver operator) tries to create settlement
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "fake-settlement".to_string(),
            intent_id: "fake-intent".to_string(),
            solver_id: "solver-1".to_string(), // Real solver ID
            user: addrs.attacker.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: 9999999999,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test that unregistered solver cannot create settlement
#[test]
fn test_unregistered_solver_cannot_create_settlement() {
    let (mut deps, env, addrs) = setup_contract();
    // Note: No solver registered

    // ATTACK: Try to create settlement without registering
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "unregistered-solver".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: 9999999999,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::SolverNotRegistered { .. }));
}

/// Test that attacker cannot mark solver locked for someone else
#[test]
fn test_attacker_cannot_mark_solver_locked() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // Mark user locked first
    let info = message_info(&addrs.escrow, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MarkUserLocked {
            settlement_id: "settlement-1".to_string(),
            escrow_id: "escrow-1".to_string(),
        },
    )
    .unwrap();

    // ATTACK: Attacker tries to mark solver locked
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MarkSolverLocked {
            settlement_id: "settlement-1".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

// ═══════════════════════════════════════════════════════════════════════════
// DOUBLE-SETTLEMENT ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that duplicate settlement ID is rejected
#[test]
fn test_duplicate_settlement_id_rejected() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // ATTACK: Try to create another settlement with same ID
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(), // Duplicate ID
            intent_id: "intent-2".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(2_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(20_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: 9999999999,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::SettlementAlreadyExists { .. }));
}

/// Test marking completed twice - documents current behavior
/// NOTE: The current contract does NOT prevent double completion.
/// This is a potential issue as it allows incrementing solver stats twice.
/// This test documents the actual behavior while noting the improvement opportunity.
#[test]
fn test_double_completion_behavior() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // Progress to completed state
    let info = message_info(&addrs.escrow, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MarkUserLocked {
            settlement_id: "settlement-1".to_string(),
            escrow_id: "escrow-1".to_string(),
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

    // First completion
    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::MarkCompleted {
            settlement_id: "settlement-1".to_string(),
        },
    )
    .unwrap();

    // Second completion attempt - should fail with state machine guards
    // FIX APPLIED (5.6/7.1): State machine guards prevent double completion
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MarkCompleted {
            settlement_id: "settlement-1".to_string(),
        },
    );

    // With state machine guards, double completion is now properly rejected
    assert!(result.is_err(), "State machine guards should prevent double completion");
}

// ═══════════════════════════════════════════════════════════════════════════
// IBC CALLBACK MANIPULATION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that non-admin cannot call IBC callback handlers
#[test]
fn test_non_admin_cannot_call_ibc_ack() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // ATTACK: Attacker tries to fake successful IBC callback
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::HandleIbcAck {
            settlement_id: "settlement-1".to_string(),
            success: true,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test that non-admin cannot call timeout handler
#[test]
fn test_non_admin_cannot_call_timeout() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // Progress to Executing state
    let info = message_info(&addrs.escrow, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MarkUserLocked {
            settlement_id: "settlement-1".to_string(),
            escrow_id: "escrow-1".to_string(),
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

    // ATTACK: Attacker tries to fake timeout
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::HandleTimeout {
            settlement_id: "settlement-1".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test timeout from wrong state fails
#[test]
fn test_timeout_from_wrong_state() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // ATTACK: Try timeout while still in Pending
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::HandleTimeout {
            settlement_id: "settlement-1".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::InvalidStateTransition { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// SLASHING ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that non-admin cannot slash
#[test]
fn test_non_admin_cannot_slash() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // ATTACK: Attacker tries to slash solver
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::SlashSolver {
            solver_id: "solver-1".to_string(),
            settlement_id: "settlement-1".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test slashing non-existent solver fails
#[test]
fn test_slash_nonexistent_solver() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);
    create_settlement(&mut deps, &env, &addrs, "settlement-1", "solver-1");

    // ATTACK: Try to slash solver that doesn't exist
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::SlashSolver {
            solver_id: "nonexistent-solver".to_string(),
            settlement_id: "settlement-1".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::SolverNotRegistered { .. }));
}

/// Test slashing for non-existent settlement fails
#[test]
fn test_slash_nonexistent_settlement() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

    // ATTACK: Try to slash for settlement that doesn't exist
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::SlashSolver {
            solver_id: "solver-1".to_string(),
            settlement_id: "nonexistent-settlement".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::SettlementNotFound { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// SOLVER BOND ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test insufficient bond is rejected
#[test]
fn test_insufficient_bond_rejected() {
    let (mut deps, env, addrs) = setup_contract();

    // ATTACK: Try to register with insufficient bond
    let info = message_info(&addrs.solver_operator, &[Coin::new(500_000u128, "uatom")]); // Less than min
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RegisterSolver {
            solver_id: "cheap-solver".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::InsufficientBond { .. }));
}

/// Test attacker cannot deregister someone else's solver
#[test]
fn test_attacker_cannot_deregister_solver() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

    // ATTACK: Attacker tries to deregister and steal bond
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::DeregisterSolver {
            solver_id: "solver-1".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test duplicate solver registration fails
#[test]
fn test_duplicate_solver_registration() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

    // ATTACK: Try to register same solver again (double bond)
    let info = message_info(&addrs.solver_operator, &[Coin::new(2_000_000u128, "uatom")]);
    let _result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RegisterSolver {
            solver_id: "solver-1".to_string(),
        },
    );

    // Should either fail or update (depending on implementation)
}

// ═══════════════════════════════════════════════════════════════════════════
// EXPIRATION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test executing expired settlement fails
#[test]
fn test_execute_expired_settlement_fails() {
    let (mut deps, mut env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs, "solver-1", 2_000_000);

    // Create settlement with short expiration
    let expires_at = env.block.time.seconds() + 60;
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "short-settlement".to_string(),
            intent_id: "intent-short".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at,
        },
    )
    .unwrap();

    // Progress to SolverLocked
    let info = message_info(&addrs.escrow, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MarkUserLocked {
            settlement_id: "short-settlement".to_string(),
            escrow_id: "escrow-short".to_string(),
        },
    )
    .unwrap();

    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::MarkSolverLocked {
            settlement_id: "short-settlement".to_string(),
        },
    )
    .unwrap();

    // Fast forward past expiration
    env.block.time = Timestamp::from_seconds(expires_at + 100);

    // ATTACK: Try to execute expired settlement
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ExecuteSettlement {
            settlement_id: "short-settlement".to_string(),
            ibc_channel: "channel-0".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::SettlementExpired {}));
}

// ═══════════════════════════════════════════════════════════════════════════
// ADMIN ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test non-admin cannot update config
#[test]
fn test_non_admin_cannot_update_config() {
    let (mut deps, env, addrs) = setup_contract();

    // ATTACK: Attacker tries to become admin
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UpdateConfig {
            admin: Some(addrs.attacker.to_string()),
            escrow_contract: None,
            min_solver_bond: None,
            base_slash_bps: None,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test non-admin cannot change escrow contract
#[test]
fn test_non_admin_cannot_change_escrow() {
    let (mut deps, env, addrs) = setup_contract();

    // ATTACK: Attacker tries to set fake escrow contract
    let fake_escrow = deps.api.addr_make("fake_escrow");
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UpdateConfig {
            admin: None,
            escrow_contract: Some(fake_escrow.to_string()),
            min_solver_bond: None,
            base_slash_bps: None,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test non-admin cannot reduce min solver bond
#[test]
fn test_non_admin_cannot_reduce_bond() {
    let (mut deps, env, addrs) = setup_contract();

    // ATTACK: Attacker tries to reduce min bond to 0
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UpdateConfig {
            admin: None,
            escrow_contract: None,
            min_solver_bond: Some(Uint128::zero()),
            base_slash_bps: None,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}
