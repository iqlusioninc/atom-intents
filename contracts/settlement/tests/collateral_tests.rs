/// Tests for collateral management and bond pool functionality
///
/// These tests cover:
/// - Collateral deposits and withdrawals
/// - Per-settlement 1.5x collateral locking
/// - Collateral unlocking on settlement completion
/// - Slashing and liquidation for failed settlements
/// - Multi-asset collateral (Native, LSM, Hydro)
/// - Pool info caching for cross-chain assets
/// - Liquidation auction mechanics

use atom_intents_types::collateral::{AssetClass, CollateralAsset};
use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
use cosmwasm_std::{from_json, Addr, Coin, Timestamp, Uint128};

use atom_intents_settlement_contract::contract::{execute, instantiate, query};
use atom_intents_settlement_contract::error::ContractError;
use atom_intents_settlement_contract::msg::{
    AvailableCollateralResponse, BondPoolResponse, CollateralValueResponse, ExecuteMsg,
    InstantiateMsg, LiquidationResponse, LiquidationsResponse, PoolInfoResponse, QueryMsg,
};

// ═══════════════════════════════════════════════════════════════════════════
// TEST HELPERS
// ═══════════════════════════════════════════════════════════════════════════

struct TestAddrs {
    admin: Addr,
    escrow: Addr,
    solver_operator: Addr,
    solver2_operator: Addr,
    user: Addr,
    attacker: Addr,
}

fn test_addrs(api: &MockApi) -> TestAddrs {
    TestAddrs {
        admin: api.addr_make("admin"),
        escrow: api.addr_make("escrow"),
        solver_operator: api.addr_make("solver_operator"),
        solver2_operator: api.addr_make("solver2_operator"),
        user: api.addr_make("user"),
        attacker: api.addr_make("attacker"),
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
    operator: &Addr,
    solver_id: &str,
    bond: u128,
) {
    let info = message_info(operator, &[Coin::new(bond, "uatom")]);
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

fn deposit_native_collateral(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    env: &cosmwasm_std::Env,
    operator: &Addr,
    solver_id: &str,
    amount: u128,
) {
    let info = message_info(operator, &[Coin::new(amount, "uatom")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::DepositCollateral {
            solver_id: solver_id.to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
        },
    )
    .unwrap();
}

fn deposit_lsm_collateral(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    env: &cosmwasm_std::Env,
    operator: &Addr,
    solver_id: &str,
    validator: &str,
    amount: u128,
) {
    let share_denom = format!("{}/1", validator);
    let info = message_info(operator, &[Coin::new(amount, &share_denom)]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::DepositCollateral {
            solver_id: solver_id.to_string(),
            asset: CollateralAsset::LsmShare {
                validator: validator.to_string(),
                share_denom,
            },
        },
    )
    .unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL DEPOSIT TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_deposit_native_collateral() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Deposit native ATOM collateral
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Query bond pool
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let result: BondPoolResponse =
        from_json(query(deps.as_ref(), env.clone(), query_msg).unwrap()).unwrap();

    assert_eq!(result.solver_id, "solver-1");
    assert_eq!(result.deposits.len(), 1);
    assert_eq!(result.deposits[0].total_amount, Uint128::new(10_000_000));
    assert_eq!(result.deposits[0].locked_amount, Uint128::zero());
    assert_eq!(result.deposits[0].available_amount, Uint128::new(10_000_000));
    assert_eq!(result.deposits[0].haircut_bps, 0); // Native has 0% haircut
}

#[test]
fn test_deposit_lsm_collateral() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Deposit LSM share collateral
    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        5_000_000,
    );

    // Query bond pool
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let result: BondPoolResponse =
        from_json(query(deps.as_ref(), env.clone(), query_msg).unwrap()).unwrap();

    assert_eq!(result.deposits.len(), 1);
    assert_eq!(result.deposits[0].total_amount, Uint128::new(5_000_000));
    assert_eq!(result.deposits[0].haircut_bps, 1000); // LSM has 10% haircut
    assert_eq!(result.deposits[0].asset_class, AssetClass::LsmShare);
}

#[test]
fn test_deposit_multiple_asset_types() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Deposit both native and LSM
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);
    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        5_000_000,
    );

    // Query bond pool
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let result: BondPoolResponse =
        from_json(query(deps.as_ref(), env.clone(), query_msg).unwrap()).unwrap();

    assert_eq!(result.deposits.len(), 2);
}

#[test]
fn test_deposit_unauthorized_fails() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Attacker tries to deposit to someone else's solver
    let info = message_info(&addrs.attacker, &[Coin::new(10_000_000u128, "uatom")]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::DepositCollateral {
            solver_id: "solver-1".to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

#[test]
fn test_deposit_to_nonexistent_solver_fails() {
    let (mut deps, env, addrs) = setup_contract();

    let info = message_info(&addrs.solver_operator, &[Coin::new(10_000_000u128, "uatom")]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::DepositCollateral {
            solver_id: "nonexistent".to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
        },
    );

    assert!(matches!(
        result.unwrap_err(),
        ContractError::SolverNotRegistered { .. }
    ));
}

#[test]
fn test_deposit_zero_amount_fails() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Try to deposit with no funds
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::DepositCollateral {
            solver_id: "solver-1".to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
        },
    );

    assert!(matches!(
        result.unwrap_err(),
        ContractError::InsufficientFunds { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL WITHDRAWAL TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_withdraw_unlocked_collateral() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Withdraw some collateral
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::WithdrawCollateral {
            solver_id: "solver-1".to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
            amount: Uint128::new(3_000_000),
        },
    );

    assert!(result.is_ok());

    // Check remaining balance
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let pool: BondPoolResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(pool.deposits[0].total_amount, Uint128::new(7_000_000));
}

#[test]
fn test_withdraw_unauthorized_fails() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Attacker tries to withdraw
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::WithdrawCollateral {
            solver_id: "solver-1".to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
            amount: Uint128::new(5_000_000),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

#[test]
fn test_withdraw_more_than_available_fails() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Try to withdraw more than deposited
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::WithdrawCollateral {
            solver_id: "solver-1".to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
            amount: Uint128::new(15_000_000),
        },
    );

    assert!(matches!(
        result.unwrap_err(),
        ContractError::InsufficientCollateral { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// COLLATERAL VALUE QUERY TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_query_collateral_value_with_haircuts() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Deposit native (0% haircut) and LSM (10% haircut)
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);
    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        10_000_000,
    );

    // Query collateral value
    let query_msg = QueryMsg::CollateralValue {
        solver_id: "solver-1".to_string(),
    };
    let result: CollateralValueResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    // Native: 10M * (1 - 0%) = 10M
    // LSM: 10M * (1 - 10%) = 9M
    // Total: 19M
    assert_eq!(result.total_value, Uint128::new(19_000_000));
    assert_eq!(result.by_class.len(), 2);
}

#[test]
fn test_query_available_collateral() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Query available collateral
    let query_msg = QueryMsg::AvailableCollateral {
        solver_id: "solver-1".to_string(),
    };
    let result: AvailableCollateralResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(result.available_value, Uint128::new(10_000_000));
    assert!(result.can_accept_settlements);
}

#[test]
fn test_query_empty_bond_pool() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Query bond pool without deposits
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let result: BondPoolResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(result.deposits.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// SETTLEMENT WITH COLLATERAL LOCKING TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_create_settlement_locks_collateral() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Create settlement (will lock 1.5x of 1M = 1.5M)
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Check collateral is locked
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let result: BondPoolResponse =
        from_json(query(deps.as_ref(), env.clone(), query_msg).unwrap()).unwrap();

    // 1.5x of 1M = 1.5M locked
    assert_eq!(result.deposits[0].locked_amount, Uint128::new(1_500_000));
    assert_eq!(result.deposits[0].available_amount, Uint128::new(8_500_000));
}

#[test]
fn test_create_settlement_insufficient_collateral_fails() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 1_000_000);

    // Try to create settlement requiring more collateral than available
    // 1.5x of 1M = 1.5M required, but only 1M available
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: 9999999999,
        },
    );

    assert!(matches!(
        result.unwrap_err(),
        ContractError::InsufficientCollateral { .. }
    ));
}

#[test]
fn test_settlement_completion_unlocks_collateral() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Create settlement
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress to completed
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

    // Check collateral is unlocked
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let result: BondPoolResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(result.deposits[0].locked_amount, Uint128::zero());
    assert_eq!(result.deposits[0].available_amount, Uint128::new(10_000_000));
}

#[test]
fn test_multiple_settlements_lock_collateral_cumulatively() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Create first settlement (locks 1.5M)
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Create second settlement (locks another 1.5M)
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-2".to_string(),
            intent_id: "intent-2".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Check cumulative locking
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let result: BondPoolResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(result.deposits[0].locked_amount, Uint128::new(3_000_000)); // 1.5M + 1.5M
    assert_eq!(result.deposits[0].available_amount, Uint128::new(7_000_000));
}

// ═══════════════════════════════════════════════════════════════════════════
// SLASHING AND LIQUIDATION TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_slash_native_collateral() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 10_000_000);

    // Create settlement
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress to Executing state for slashing
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

    // Slash the solver
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SlashSolver {
            solver_id: "solver-1".to_string(),
            settlement_id: "settlement-1".to_string(),
        },
    );

    assert!(result.is_ok());
    let response = result.unwrap();

    // Check the slashing happened
    assert!(response
        .attributes
        .iter()
        .any(|a| a.key == "slash_type" && a.value == "native"));

    // Check collateral was seized (no longer in pool)
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let pool: BondPoolResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    // 10M - 1.5M slashed = 8.5M remaining
    assert_eq!(pool.deposits[0].total_amount, Uint128::new(8_500_000));
}

#[test]
fn test_slash_lsm_creates_liquidation() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Only deposit LSM (no native) to force LSM usage
    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        10_000_000,
    );

    // Create settlement
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress through states
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

    // Slash the solver - should create liquidation
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SlashSolver {
            solver_id: "solver-1".to_string(),
            settlement_id: "settlement-1".to_string(),
        },
    );

    assert!(result.is_ok());
    let response = result.unwrap();

    // Check liquidation was created
    assert!(response
        .attributes
        .iter()
        .any(|a| a.key == "slash_type" && a.value == "liquidation"));

    // Query the liquidation
    let query_msg = QueryMsg::Liquidation {
        liquidation_id: "liq-settlement-1".to_string(),
    };
    let liq: LiquidationResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(liq.source_settlement_id, "settlement-1");
    assert_eq!(liq.slashed_solver, "solver-1");
    assert_eq!(liq.min_output_amount, Uint128::new(1_000_000));
}

// ═══════════════════════════════════════════════════════════════════════════
// POOL INFO CACHING TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_update_pool_info() {
    let (mut deps, env, addrs) = setup_contract();

    // Admin updates pool info (simulating ICQ callback)
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdatePoolInfo {
            share_denom: "hydro/pool/1".to_string(),
            total_shares_issued: Uint128::new(1_000_000_000),
            total_pool_value: Uint128::new(1_100_000_000), // 1.1x value
        },
    );

    assert!(result.is_ok());

    // Query the cached info
    let query_msg = QueryMsg::CachedPoolInfo {
        share_denom: "hydro/pool/1".to_string(),
    };
    let info: PoolInfoResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(info.share_denom, "hydro/pool/1");
    assert_eq!(info.total_shares_issued, Uint128::new(1_000_000_000));
    assert_eq!(info.total_pool_value, Uint128::new(1_100_000_000));
    assert!(!info.is_stale);
}

#[test]
fn test_update_pool_info_unauthorized() {
    let (mut deps, env, addrs) = setup_contract();

    // Non-admin tries to update pool info
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UpdatePoolInfo {
            share_denom: "hydro/pool/1".to_string(),
            total_shares_issued: Uint128::new(1_000_000_000),
            total_pool_value: Uint128::new(1_100_000_000),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

#[test]
fn test_pool_info_staleness() {
    let (mut deps, mut env, addrs) = setup_contract();

    // Update pool info
    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdatePoolInfo {
            share_denom: "hydro/pool/1".to_string(),
            total_shares_issued: Uint128::new(1_000_000_000),
            total_pool_value: Uint128::new(1_100_000_000),
        },
    )
    .unwrap();

    // Query immediately - should not be stale
    let query_msg = QueryMsg::CachedPoolInfo {
        share_denom: "hydro/pool/1".to_string(),
    };
    let info: PoolInfoResponse =
        from_json(query(deps.as_ref(), env.clone(), query_msg.clone()).unwrap()).unwrap();
    assert!(!info.is_stale);

    // Fast forward past staleness limit (1 hour)
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 3601);

    // Query again - should be stale
    let info: PoolInfoResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();
    assert!(info.is_stale);
}

// ═══════════════════════════════════════════════════════════════════════════
// LIQUIDATION AUCTION TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_bid_on_liquidation() {
    let (mut deps, env, addrs) = setup_contract();

    // Set up a liquidation (reuse slash_lsm setup)
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    register_solver(&mut deps, &env, &addrs.solver2_operator, "solver-2", 2_000_000);

    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        10_000_000,
    );

    // Create and progress settlement
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress to Executing
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

    // Slash to create liquidation
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

    // Another solver bids on the liquidation
    let info = message_info(&addrs.solver2_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::BidOnLiquidation {
            liquidation_id: "liq-settlement-1".to_string(),
            offered_output: Uint128::new(1_000_000),
        },
    );

    assert!(result.is_ok());
}

#[test]
fn test_bid_below_minimum_fails() {
    let (mut deps, env, addrs) = setup_contract();

    // Same setup as above
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    register_solver(&mut deps, &env, &addrs.solver2_operator, "solver-2", 2_000_000);

    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        10_000_000,
    );

    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress through states
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

    // Bid below minimum (min is 1M, bid 500k)
    let info = message_info(&addrs.solver2_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::BidOnLiquidation {
            liquidation_id: "liq-settlement-1".to_string(),
            offered_output: Uint128::new(500_000),
        },
    );

    assert!(matches!(
        result.unwrap_err(),
        ContractError::BidBelowMinimum { .. }
    ));
}

#[test]
fn test_execute_liquidation() {
    let (mut deps, env, addrs) = setup_contract();

    // Same setup as above
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    register_solver(&mut deps, &env, &addrs.solver2_operator, "solver-2", 2_000_000);

    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        10_000_000,
    );

    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress through states
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

    // Execute liquidation with enough ATOM
    let info = message_info(&addrs.solver2_operator, &[Coin::new(1_000_000u128, "uatom")]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ExecuteLiquidation {
            liquidation_id: "liq-settlement-1".to_string(),
        },
    );

    assert!(result.is_ok());
    let response = result.unwrap();

    // Check bank messages for transfers
    assert_eq!(response.messages.len(), 2); // ATOM to beneficiary, collateral to liquidator
}

#[test]
fn test_execute_liquidation_insufficient_funds() {
    let (mut deps, env, addrs) = setup_contract();

    // Same setup
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    register_solver(&mut deps, &env, &addrs.solver2_operator, "solver-2", 2_000_000);

    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        10_000_000,
    );

    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress through states
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

    // Try to execute with insufficient ATOM
    let info = message_info(&addrs.solver2_operator, &[Coin::new(500_000u128, "uatom")]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ExecuteLiquidation {
            liquidation_id: "liq-settlement-1".to_string(),
        },
    );

    assert!(matches!(
        result.unwrap_err(),
        ContractError::InsufficientFunds { .. }
    ));
}

#[test]
fn test_cancel_liquidation() {
    let (mut deps, env, addrs) = setup_contract();

    // Same setup
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        10_000_000,
    );

    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress through states
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

    // Admin cancels the liquidation
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CancelLiquidation {
            liquidation_id: "liq-settlement-1".to_string(),
            reason: "No bids received".to_string(),
        },
    );

    assert!(result.is_ok());
}

#[test]
fn test_cancel_liquidation_unauthorized() {
    let (mut deps, env, addrs) = setup_contract();

    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        10_000_000,
    );

    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Progress through states
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

    // Attacker tries to cancel
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CancelLiquidation {
            liquidation_id: "liq-settlement-1".to_string(),
            reason: "Stealing collateral".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

// ═══════════════════════════════════════════════════════════════════════════
// EDGE CASE TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_cannot_withdraw_locked_collateral() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Create settlement (locks 1.5M of 2M)
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Try to withdraw more than unlocked (only 500k available)
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::WithdrawCollateral {
            solver_id: "solver-1".to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
            amount: Uint128::new(1_000_000), // Trying to withdraw 1M when only 500k unlocked
        },
    );

    assert!(matches!(
        result.unwrap_err(),
        ContractError::InsufficientCollateral { .. }
    ));
}

#[test]
fn test_withdraw_up_to_available() {
    let (mut deps, env, addrs) = setup_contract();
    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);
    deposit_native_collateral(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    // Create settlement (locks 1.5M of 2M)
    let info = message_info(&addrs.solver_operator, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateSettlement {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user: addrs.user.to_string(),
            user_input_amount: Uint128::new(1_000_000),
            user_input_denom: "uatom".to_string(),
            solver_output_amount: Uint128::new(10_000_000),
            solver_output_denom: "uusdc".to_string(),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();

    // Withdraw exactly the available amount (500k)
    let info = message_info(&addrs.solver_operator, &[]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::WithdrawCollateral {
            solver_id: "solver-1".to_string(),
            asset: CollateralAsset::Native {
                denom: "uatom".to_string(),
            },
            amount: Uint128::new(500_000),
        },
    );

    assert!(result.is_ok());

    // Verify remaining
    let query_msg = QueryMsg::BondPool {
        solver_id: "solver-1".to_string(),
    };
    let pool: BondPoolResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(pool.deposits[0].total_amount, Uint128::new(1_500_000));
    assert_eq!(pool.deposits[0].locked_amount, Uint128::new(1_500_000));
    assert_eq!(pool.deposits[0].available_amount, Uint128::zero());
}

#[test]
fn test_query_pending_liquidations() {
    let (mut deps, env, addrs) = setup_contract();

    register_solver(&mut deps, &env, &addrs.solver_operator, "solver-1", 2_000_000);

    deposit_lsm_collateral(
        &mut deps,
        &env,
        &addrs.solver_operator,
        "solver-1",
        "cosmosvaloper1xxx",
        20_000_000,
    );

    // Create two settlements
    for i in 1..=2 {
        let info = message_info(&addrs.solver_operator, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::CreateSettlement {
                settlement_id: format!("settlement-{}", i),
                intent_id: format!("intent-{}", i),
                solver_id: "solver-1".to_string(),
                user: addrs.user.to_string(),
                user_input_amount: Uint128::new(1_000_000),
                user_input_denom: "uatom".to_string(),
                solver_output_amount: Uint128::new(10_000_000),
                solver_output_denom: "uusdc".to_string(),
                expires_at: env.block.time.seconds() + 3600,
            },
        )
        .unwrap();

        // Progress through states
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

        // Slash to create liquidation
        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::SlashSolver {
                solver_id: "solver-1".to_string(),
                settlement_id: format!("settlement-{}", i),
            },
        )
        .unwrap();
    }

    // Query pending liquidations
    let query_msg = QueryMsg::PendingLiquidations {
        start_after: None,
        limit: Some(10),
    };
    let result: LiquidationsResponse =
        from_json(query(deps.as_ref(), env, query_msg).unwrap()).unwrap();

    assert_eq!(result.liquidations.len(), 2);
}
