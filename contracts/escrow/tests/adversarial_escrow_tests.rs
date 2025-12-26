/// Adversarial tests for the escrow contract
///
/// These tests simulate attacks where things could go horribly wrong:
/// - Double-release/double-refund attacks
/// - Unauthorized release attempts
/// - Race conditions between release and refund
/// - Funds stuck scenarios
/// - Admin takeover attacks
/// - Time manipulation attacks

use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
use cosmwasm_std::{from_json, Addr, Coin, Timestamp};

use atom_intents_escrow::contract::{execute, instantiate, query};
use atom_intents_escrow::error::ContractError;
use atom_intents_escrow::msg::{EscrowResponse, ExecuteMsg, InstantiateMsg, QueryMsg};

// Helper to get test addresses using MockApi
struct TestAddrs {
    admin: Addr,
    settlement: Addr,
    user: Addr,
    recipient: Addr,
    attacker: Addr,
    fake_settlement: Addr,
}

fn test_addrs(api: &MockApi) -> TestAddrs {
    TestAddrs {
        admin: api.addr_make("admin"),
        settlement: api.addr_make("settlement"),
        user: api.addr_make("user"),
        recipient: api.addr_make("recipient"),
        attacker: api.addr_make("attacker"),
        fake_settlement: api.addr_make("fake_settlement"),
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
        settlement_contract: addrs.settlement.to_string(),
    };
    let info = message_info(&addrs.admin, &[]);

    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    (deps, env, addrs)
}

fn lock_escrow(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    env: &cosmwasm_std::Env,
    addrs: &TestAddrs,
    escrow_id: &str,
    amount: u128,
) {
    let info = message_info(&addrs.user, &[Coin::new(amount, "uatom")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Lock {
            escrow_id: escrow_id.to_string(),
            intent_id: format!("intent_{}", escrow_id),
            expires_at: env.block.time.seconds() + 3600,
        },
    )
    .unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// DOUBLE-RELEASE ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that releasing the same escrow twice fails
#[test]
fn test_double_release_attack_fails() {
    let (mut deps, env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // First release succeeds
    let info = message_info(&addrs.settlement, &[]);
    let result1 = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::Release {
            escrow_id: "escrow-1".to_string(),
            recipient: addrs.recipient.to_string(),
        },
    );
    assert!(result1.is_ok());

    // ATTACK: Second release must fail
    let result2 = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Release {
            escrow_id: "escrow-1".to_string(),
            recipient: addrs.attacker.to_string(), // Attacker tries to redirect
        },
    );
    assert!(result2.is_err());
}

/// Test that releasing then refunding fails
#[test]
fn test_release_then_refund_fails() {
    let (mut deps, mut env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // Release first
    let info = message_info(&addrs.settlement, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Release {
            escrow_id: "escrow-1".to_string(),
            recipient: addrs.recipient.to_string(),
        },
    )
    .unwrap();

    // Fast forward past expiration
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

    // ATTACK: Try to refund after release
    let info = message_info(&addrs.user, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    );

    // MUST fail - funds already released
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// DOUBLE-REFUND ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that refunding the same escrow twice fails
#[test]
fn test_double_refund_attack_fails() {
    let (mut deps, mut env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // Fast forward past expiration
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

    // First refund succeeds
    let info = message_info(&addrs.user, &[]);
    let result1 = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    );
    assert!(result1.is_ok());

    // ATTACK: Second refund must fail
    let result2 = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    );
    assert!(result2.is_err());
}

/// Test that refunding then releasing fails
#[test]
fn test_refund_then_release_fails() {
    let (mut deps, mut env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // Fast forward and refund first
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);
    let info = message_info(&addrs.user, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    )
    .unwrap();

    // ATTACK: Try to release after refund
    let info = message_info(&addrs.settlement, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Release {
            escrow_id: "escrow-1".to_string(),
            recipient: addrs.attacker.to_string(),
        },
    );

    // MUST fail - funds already refunded
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// UNAUTHORIZED ACCESS ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that random user cannot release escrow
#[test]
fn test_random_user_cannot_release() {
    let (mut deps, env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // ATTACK: Random user tries to release
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Release {
            escrow_id: "escrow-1".to_string(),
            recipient: addrs.attacker.to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test that fake settlement contract cannot release
#[test]
fn test_fake_settlement_cannot_release() {
    let (mut deps, env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // ATTACK: Fake settlement contract tries to release
    let info = message_info(&addrs.fake_settlement, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Release {
            escrow_id: "escrow-1".to_string(),
            recipient: addrs.attacker.to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test that random user cannot refund someone else's escrow
#[test]
fn test_random_user_cannot_refund_others_escrow() {
    let (mut deps, mut env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // Fast forward past expiration
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

    // ATTACK: Attacker tries to refund (even though expired)
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

// ═══════════════════════════════════════════════════════════════════════════
// TIME MANIPULATION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that early refund is blocked
#[test]
fn test_early_refund_blocked() {
    let (mut deps, env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // ATTACK: Try to refund immediately (before expiration)
    let info = message_info(&addrs.user, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::EscrowNotExpired { .. }));
}

/// Test refund at exact expiration time succeeds (boundary condition)
#[test]
fn test_refund_at_exact_expiration() {
    let (mut deps, mut env, addrs) = setup_contract();

    // Lock with specific expiration
    let expires_at = env.block.time.seconds() + 3600;
    let info = message_info(&addrs.user, &[Coin::new(1_000_000u128, "uatom")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Lock {
            escrow_id: "escrow-1".to_string(),
            intent_id: "intent-1".to_string(),
            expires_at,
        },
    )
    .unwrap();

    // Set time to exactly expiration (boundary condition)
    env.block.time = Timestamp::from_seconds(expires_at);

    // At exactly expiration time, refund SHOULD succeed
    // The contract checks `time < expires_at`, so when `time == expires_at`
    // the check passes and refund is allowed
    let info = message_info(&addrs.user, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    );

    // Refund succeeds at exactly expiration time
    assert!(result.is_ok());
}

/// Test refund one second after expiration
#[test]
fn test_refund_one_second_after_expiration() {
    let (mut deps, mut env, addrs) = setup_contract();

    let expires_at = env.block.time.seconds() + 3600;
    let info = message_info(&addrs.user, &[Coin::new(1_000_000u128, "uatom")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Lock {
            escrow_id: "escrow-1".to_string(),
            intent_id: "intent-1".to_string(),
            expires_at,
        },
    )
    .unwrap();

    // Set time to one second after expiration
    env.block.time = Timestamp::from_seconds(expires_at + 1);

    let info = message_info(&addrs.user, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    );

    // Should succeed now
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// ADMIN TAKEOVER ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that non-admin cannot update config
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
            settlement_contract: None,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test that attacker cannot change settlement contract
#[test]
fn test_attacker_cannot_change_settlement_contract() {
    let (mut deps, env, addrs) = setup_contract();

    // ATTACK: Attacker tries to set their contract as settlement
    let info = message_info(&addrs.attacker, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UpdateConfig {
            admin: None,
            settlement_contract: Some(addrs.fake_settlement.to_string()),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));
}

/// Test admin transfer is successful
#[test]
fn test_admin_transfer_successful() {
    let (mut deps, env, addrs) = setup_contract();
    let new_admin = deps.api.addr_make("new_admin");

    // Admin transfers admin rights
    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateConfig {
            admin: Some(new_admin.to_string()),
            settlement_contract: None,
        },
    )
    .unwrap();

    // Old admin can no longer update config
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateConfig {
            admin: Some(addrs.admin.to_string()),
            settlement_contract: None,
        },
    );
    assert!(matches!(result.unwrap_err(), ContractError::Unauthorized {}));

    // New admin can update config
    let info = message_info(&new_admin, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UpdateConfig {
            admin: None,
            settlement_contract: Some(addrs.fake_settlement.to_string()),
        },
    );
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// FUNDS STUCK ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that funds can always be recovered via refund after expiration
#[test]
fn test_funds_recoverable_after_expiration() {
    let (mut deps, mut env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // Even if settlement contract disappears, user can still refund after expiration
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

    let info = message_info(&addrs.user, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Refund {
            escrow_id: "escrow-1".to_string(),
        },
    );

    assert!(result.is_ok());
}

/// Test escrow with very long expiration
#[test]
fn test_long_expiration_escrow() {
    let (mut deps, env, addrs) = setup_contract();

    // Lock with 100 year expiration (potential funds stuck)
    let far_future = env.block.time.seconds() + (100 * 365 * 24 * 3600);
    let info = message_info(&addrs.user, &[Coin::new(1_000_000u128, "uatom")]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Lock {
            escrow_id: "escrow-long".to_string(),
            intent_id: "intent-long".to_string(),
            expires_at: far_future,
        },
    );

    // Should succeed (user's choice to lock for long time)
    assert!(result.is_ok());

    // But settlement contract can still release before expiration
    let info = message_info(&addrs.settlement, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Release {
            escrow_id: "escrow-long".to_string(),
            recipient: addrs.recipient.to_string(),
        },
    );
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// INPUT VALIDATION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that duplicate escrow ID is rejected
#[test]
fn test_duplicate_escrow_id_rejected() {
    let (mut deps, env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // ATTACK: Try to lock with same ID
    let info = message_info(&addrs.attacker, &[Coin::new(2_000_000u128, "uatom")]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Lock {
            escrow_id: "escrow-1".to_string(),
            intent_id: "intent-attacker".to_string(),
            expires_at: 9999999999,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::EscrowAlreadyExists { .. }));
}

/// Test locking with multiple coins fails
#[test]
fn test_lock_multiple_coins_fails() {
    let (mut deps, env, addrs) = setup_contract();

    // ATTACK: Send multiple coins
    let info = message_info(
        &addrs.user,
        &[
            Coin::new(1_000_000u128, "uatom"),
            Coin::new(500_000u128, "uusdc"),
        ],
    );
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Lock {
            escrow_id: "escrow-multi".to_string(),
            intent_id: "intent-multi".to_string(),
            expires_at: 9999999999,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::InvalidFunds { .. }));
}

/// Test locking with no coins fails
#[test]
fn test_lock_no_coins_fails() {
    let (mut deps, env, addrs) = setup_contract();

    // ATTACK: Send no coins
    let info = message_info(&addrs.user, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Lock {
            escrow_id: "escrow-empty".to_string(),
            intent_id: "intent-empty".to_string(),
            expires_at: 9999999999,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::InvalidFunds { .. }));
}

/// Test release to invalid address is handled
#[test]
fn test_release_invalid_recipient() {
    let (mut deps, env, addrs) = setup_contract();
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // Try to release to empty address
    let info = message_info(&addrs.settlement, &[]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Release {
            escrow_id: "escrow-1".to_string(),
            recipient: "".to_string(),
        },
    );

    // Should fail on address validation
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// LOCK FROM IBC (CROSS-CHAIN ESCROW) TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test LockFromIbc with valid IBC funds
#[test]
fn test_lock_from_ibc_success() {
    let (mut deps, env, addrs) = setup_contract();

    // Send IBC funds (denom starts with "ibc/")
    let ibc_denom = "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2";
    let info = message_info(&addrs.user, &[Coin::new(1_000_000u128, ibc_denom)]);

    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::LockFromIbc {
            intent_id: "intent_celestia_123".to_string(),
            expires_at: env.block.time.seconds() + 3600,
            user_source_address: "celestia1user123abc".to_string(),
            source_chain_id: "celestia".to_string(),
            source_channel: "channel-0".to_string(),
        },
    );

    assert!(result.is_ok());

    // Verify escrow was created with cross-chain fields
    let escrow: EscrowResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EscrowByIntent {
                intent_id: "intent_celestia_123".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(escrow.owner_chain_id, Some("celestia".to_string()));
    assert_eq!(escrow.owner_source_address, Some("celestia1user123abc".to_string()));
    assert_eq!(escrow.source_channel, Some("channel-0".to_string()));
    assert!(escrow.denom.starts_with("ibc/"));
}

/// Test LockFromIbc fails without IBC denom
#[test]
fn test_lock_from_ibc_requires_ibc_denom() {
    let (mut deps, env, addrs) = setup_contract();

    // ATTACK: Try to use LockFromIbc with non-IBC funds
    let info = message_info(&addrs.user, &[Coin::new(1_000_000u128, "uatom")]);

    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::LockFromIbc {
            intent_id: "intent_attack_123".to_string(),
            expires_at: env.block.time.seconds() + 3600,
            user_source_address: "celestia1attacker".to_string(),
            source_chain_id: "celestia".to_string(),
            source_channel: "channel-0".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::NotIbcFunds {}));
}

/// Test LockFromIbc replay protection (same intent_id fails)
#[test]
fn test_lock_from_ibc_replay_protection() {
    let (mut deps, env, addrs) = setup_contract();

    let ibc_denom = "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2";

    // First lock succeeds
    let info = message_info(&addrs.user, &[Coin::new(1_000_000u128, ibc_denom)]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::LockFromIbc {
            intent_id: "intent_replay_test".to_string(),
            expires_at: env.block.time.seconds() + 3600,
            user_source_address: "celestia1user".to_string(),
            source_chain_id: "celestia".to_string(),
            source_channel: "channel-0".to_string(),
        },
    )
    .unwrap();

    // ATTACK: Second lock with same intent_id must fail
    let info = message_info(&addrs.attacker, &[Coin::new(2_000_000u128, ibc_denom)]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::LockFromIbc {
            intent_id: "intent_replay_test".to_string(),
            expires_at: 9999999999,
            user_source_address: "celestia1attacker".to_string(),
            source_chain_id: "celestia".to_string(),
            source_channel: "channel-fake".to_string(),
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::IntentAlreadyEscrowed { .. }));
}

/// Test cross-chain escrow refund initiates IBC transfer
#[test]
fn test_cross_chain_refund_uses_ibc() {
    let (mut deps, mut env, addrs) = setup_contract();

    let ibc_denom = "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2";
    let info = message_info(&addrs.user, &[Coin::new(1_000_000u128, ibc_denom)]);

    // Lock via IBC
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::LockFromIbc {
            intent_id: "intent_refund_test".to_string(),
            expires_at: env.block.time.seconds() + 3600,
            user_source_address: "celestia1userrefund".to_string(),
            source_chain_id: "celestia".to_string(),
            source_channel: "channel-0".to_string(),
        },
    )
    .unwrap();

    // Fast forward past expiration
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

    // Admin triggers refund (for cross-chain, admin can also trigger)
    let info = message_info(&addrs.admin, &[]);
    let result = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Refund {
            escrow_id: "esc_intent_refund_test".to_string(),
        },
    );

    assert!(result.is_ok());

    // The response should contain an IBC transfer message
    let response = result.unwrap();
    assert_eq!(response.messages.len(), 1);

    // Verify escrow status is now Refunding (waiting for IBC ack)
    let escrow: EscrowResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::Escrow {
                escrow_id: "esc_intent_refund_test".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(escrow.status, "refunding");
}

/// Test that duplicate intent_id is rejected for regular Lock too
#[test]
fn test_duplicate_intent_id_rejected() {
    let (mut deps, env, addrs) = setup_contract();

    // First lock with intent_id
    lock_escrow(&mut deps, &env, &addrs, "escrow-1", 1_000_000);

    // ATTACK: Try to lock with different escrow_id but same intent_id
    let info = message_info(&addrs.attacker, &[Coin::new(2_000_000u128, "uatom")]);
    let result = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Lock {
            escrow_id: "escrow-attacker".to_string(),
            intent_id: "intent_escrow-1".to_string(), // Same intent_id as first lock
            expires_at: 9999999999,
        },
    );

    assert!(matches!(result.unwrap_err(), ContractError::IntentAlreadyEscrowed { .. }));
}
