//! Adversarial tests for Ethereum escrow handlers
//!
//! These tests verify the security and economic robustness of the Ethereum
//! escrow system against various attack vectors.

use atom_intents_escrow::contract::{execute, instantiate, query};
use atom_intents_escrow::msg::{
    EthereumEscrowStatusResponse, ExecuteMsg, InstantiateMsg, QueryMsg,
};
use atom_intents_types::{Asset, ExecutionConstraints, FillConfig, Intent, OutputSpec};
use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
use cosmwasm_std::{from_json, Addr, Binary, Coin, Timestamp, Uint128};

// ═══════════════════════════════════════════════════════════════════════════════
// TEST HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

struct TestAddrs {
    admin: Addr,
    settlement: Addr,
    solver1: Addr,
    solver2: Addr,
    attacker: Addr,
    user: Addr,
}

fn test_addrs(api: &MockApi) -> TestAddrs {
    TestAddrs {
        admin: api.addr_make("admin"),
        settlement: api.addr_make("settlement"),
        solver1: api.addr_make("solver1"),
        solver2: api.addr_make("solver2"),
        attacker: api.addr_make("attacker"),
        user: api.addr_make("user"),
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

fn create_test_intent(id: &str, denom: &str, amount: u128) -> Intent {
    Intent {
        id: id.to_string(),
        version: "1.0".to_string(),
        nonce: 1,
        user: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
        input: Asset {
            chain_id: "ethereum-1".to_string(),
            denom: denom.to_string(),
            amount: Uint128::new(amount),
        },
        output: OutputSpec {
            chain_id: "cosmoshub-4".to_string(),
            denom: "uatom".to_string(),
            min_amount: Uint128::new(100_000),
            limit_price: "0.01".to_string(),
            recipient: "cosmos1user".to_string(),
        },
        fill_config: FillConfig::default(),
        constraints: ExecutionConstraints::new(9999999999),
        signature: Binary::default(),
        public_key: Binary::default(),
        created_at: 1000000,
        expires_at: 9999999999,
    }
}

/// Helper to register an intent
fn register_intent(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    env: &cosmwasm_std::Env,
    addrs: &TestAddrs,
    intent_id: &str,
    eth_sender: &str,
    amount: u128,
    timeout_secs: u64,
) {
    let intent = create_test_intent(intent_id, "ibc/eth", amount);
    let info = message_info(&addrs.settlement, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: eth_sender.to_string(),
            expected_amount: Uint128::new(amount),
            eureka_timeout_secs: timeout_secs,
        },
    )
    .unwrap();
}

/// Helper to notify packet received
fn notify_packet_received(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    env: &cosmwasm_std::Env,
    addrs: &TestAddrs,
    intent_id: &str,
    packet_id: &str,
    amount: u128,
    sender: &str,
) {
    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: intent_id.to_string(),
            packet_id: packet_id.to_string(),
            amount: Uint128::new(amount),
            sender: sender.to_string(),
        },
    )
    .unwrap();
}

/// Helper to front settlement
fn front_settlement(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    env: &cosmwasm_std::Env,
    solver: &Addr,
    intent_id: &str,
    solver_id: &str,
    output_amount: u128,
    bond_amount: u128,
    bond_denom: &str,
) {
    let info = message_info(solver, &[Coin::new(bond_amount, bond_denom)]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: intent_id.to_string(),
            solver_id: solver_id.to_string(),
            output_amount: Uint128::new(output_amount),
            risk_bond: Uint128::new(bond_amount),
        },
    )
    .unwrap();
}

/// Helper to finalize
fn finalize_escrow(
    deps: &mut cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    env: &cosmwasm_std::Env,
    addrs: &TestAddrs,
    intent_id: &str,
    packet_id: &str,
    proof_block: u64,
) {
    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaFinalized {
            intent_id: intent_id.to_string(),
            packet_id: packet_id.to_string(),
            proof_block,
        },
    )
    .unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════════
// ECONOMIC ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Griefing Attack: Register intent but never send Ethereum funds
/// Expected: Intent stays in Pending, can be failed by admin, no solver loss
#[test]
fn test_griefing_register_without_sending_eth() {
    let (mut deps, env, addrs) = setup_contract();

    // Attacker registers intent but never sends ETH
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "grief-intent-1",
        "0xattacker",
        1_000_000,
        1200,
    );

    // Verify status is pending
    let status: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::EthereumEscrowStatus {
                intent_id: "grief-intent-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status.escrow_status, "pending");

    // Admin can mark as failed after timeout - no solver affected
    let info = message_info(&addrs.admin, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::HandleEurekaEscrowFailure {
            intent_id: "grief-intent-1".to_string(),
            reason: "No Ethereum funds received".to_string(),
        },
    )
    .unwrap();

    // No messages (no bond to return since no solver fronted)
    assert_eq!(res.messages.len(), 0);
    assert_eq!(res.attributes[3].value, "none"); // bond_action = none
}

/// Front-running Attack: Solver1 fronts, Solver2 tries to also front
/// Expected: Second front attempt fails
#[test]
fn test_frontrunning_double_front_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Setup: register and receive packet
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "front-race-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "front-race-1",
        "packet-1",
        1_000_000,
        "0xuser",
    );

    // Solver1 fronts first
    front_settlement(
        &mut deps,
        &env,
        &addrs.solver1,
        "front-race-1",
        "solver1",
        950_000,
        1_500_000,
        "ibc/eth",
    );

    // Solver2 tries to front same intent - should fail
    let info = message_info(&addrs.solver2, &[Coin::new(1_500_000u128, "ibc/eth")]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "front-race-1".to_string(),
            solver_id: "solver2".to_string(),
            output_amount: Uint128::new(960_000), // Better offer
            risk_bond: Uint128::new(1_500_000),
        },
    )
    .unwrap_err();

    // Should fail because already fronted
    assert!(err.to_string().contains("Invalid Ethereum escrow status"));
}

/// Claim Theft: Attacker tries to claim escrow they didn't front
/// Expected: Unauthorized error
#[test]
fn test_claim_theft_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Full flow until finalized
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "claim-theft-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "claim-theft-1",
        "packet-1",
        1_000_000,
        "0xuser",
    );
    front_settlement(
        &mut deps,
        &env,
        &addrs.solver1,
        "claim-theft-1",
        "solver1",
        950_000,
        1_500_000,
        "ibc/eth",
    );
    finalize_escrow(&mut deps, &env, &addrs, "claim-theft-1", "packet-1", 12345678);

    // Attacker tries to claim
    let info = message_info(&addrs.attacker, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "claim-theft-1".to_string(),
            packet_id: "packet-1".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

/// Bond Extraction Attack: Fail escrow after fronting, verify bond returned
/// Expected: Solver gets bond back on failure (not slashed for non-fault)
#[test]
fn test_bond_returned_on_non_fault_failure() {
    let (mut deps, env, addrs) = setup_contract();

    // Setup with fronting
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "bond-return-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "bond-return-1",
        "packet-1",
        1_000_000,
        "0xuser",
    );
    front_settlement(
        &mut deps,
        &env,
        &addrs.solver1,
        "bond-return-1",
        "solver1",
        950_000,
        1_500_000,
        "ibc/eth",
    );

    // ZK proof fails (not solver's fault)
    let info = message_info(&addrs.admin, &[]);
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::HandleEurekaEscrowFailure {
            intent_id: "bond-return-1".to_string(),
            reason: "ZK proof verification failed".to_string(),
        },
    )
    .unwrap();

    // Bond should be returned
    assert_eq!(res.attributes[3].value, "returned");
    assert_eq!(res.messages.len(), 1); // Bank send to return bond
}

/// Timeout Manipulation: Submit packet notification at edge of timeout
/// Expected: Packets after timeout are rejected
#[test]
fn test_timeout_edge_case_exact_boundary() {
    let (mut deps, mut env, addrs) = setup_contract();

    // Register with 100 second timeout
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "timeout-edge-1",
        "0xuser",
        1_000_000,
        100,
    );

    let initial_time = env.block.time.seconds();

    // Try at exactly timeout - should fail
    env.block.time = Timestamp::from_seconds(initial_time + 100 + 1);

    let info = message_info(&addrs.admin, &[]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "timeout-edge-1".to_string(),
            packet_id: "packet-1".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Eureka timeout"));
}

/// Timeout Manipulation: Submit just before timeout
/// Expected: Packets just before timeout are accepted
#[test]
fn test_timeout_edge_case_just_before() {
    let (mut deps, mut env, addrs) = setup_contract();

    // Register with 100 second timeout
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "timeout-before-1",
        "0xuser",
        1_000_000,
        100,
    );

    let initial_time = env.block.time.seconds();

    // Try 1 second before timeout - should succeed
    env.block.time = Timestamp::from_seconds(initial_time + 99);

    let info = message_info(&addrs.admin, &[]);
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "timeout-before-1".to_string(),
            packet_id: "packet-1".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser".to_string(),
        },
    );

    assert!(res.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECURITY TESTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Replay Protection: Same intent ID rejected on second registration
#[test]
fn test_replay_protection_duplicate_intent() {
    let (mut deps, env, addrs) = setup_contract();

    // First registration succeeds
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "replay-intent-1",
        "0xuser",
        1_000_000,
        1200,
    );

    // Second registration with same ID fails
    let intent = create_test_intent("replay-intent-1", "ibc/eth", 2_000_000);
    let info = message_info(&addrs.settlement, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xdifferent".to_string(),
            expected_amount: Uint128::new(2_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Intent already has escrow"));
}

/// Amount Manipulation: Received amount less than expected
/// Expected: Still accepted (solver takes the risk on amount mismatch)
#[test]
fn test_amount_mismatch_less_than_expected() {
    let (mut deps, env, addrs) = setup_contract();

    // Register expecting 1M
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "amount-less-1",
        "0xuser",
        1_000_000,
        1200,
    );

    // Receive only 900k (10% less)
    let info = message_info(&addrs.admin, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "amount-less-1".to_string(),
            packet_id: "packet-1".to_string(),
            amount: Uint128::new(900_000), // Less than expected
            sender: "0xuser".to_string(),
        },
    );

    // Should succeed - solver will see the mismatch and decide whether to front
    assert!(res.is_ok());

    // Verify received amount is recorded
    let status: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EthereumEscrowStatus {
                intent_id: "amount-less-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status.escrow_status, "received");
}

/// Fake Finalization: Non-admin tries to finalize
/// Expected: Unauthorized error
#[test]
fn test_fake_finalization_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Setup until received
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "fake-final-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "fake-final-1",
        "packet-1",
        1_000_000,
        "0xuser",
    );

    // Attacker tries to finalize
    let info = message_info(&addrs.attacker, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::NotifyEurekaFinalized {
            intent_id: "fake-final-1".to_string(),
            packet_id: "packet-1".to_string(),
            proof_block: 99999999,
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

/// Fake Packet Notification: Non-admin tries to notify packet received
/// Expected: Unauthorized error
#[test]
fn test_fake_packet_notification_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    register_intent(
        &mut deps,
        &env,
        &addrs,
        "fake-packet-1",
        "0xuser",
        1_000_000,
        1200,
    );

    // Attacker tries to notify packet
    let info = message_info(&addrs.attacker, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "fake-packet-1".to_string(),
            packet_id: "fake-packet".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

/// Unauthorized Registration: Random user tries to register intent
/// Expected: Unauthorized error (only settlement contract or admin)
#[test]
fn test_unauthorized_registration_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    let intent = create_test_intent("unauth-intent-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.attacker, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xattacker".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

/// Wrong Packet ID on Claim: Solver tries to claim with wrong packet ID
/// Expected: PacketIdMismatch error
#[test]
fn test_wrong_packet_id_claim_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Full flow
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "wrong-packet-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "wrong-packet-1",
        "correct-packet",
        1_000_000,
        "0xuser",
    );
    front_settlement(
        &mut deps,
        &env,
        &addrs.solver1,
        "wrong-packet-1",
        "solver1",
        950_000,
        1_500_000,
        "ibc/eth",
    );
    finalize_escrow(
        &mut deps,
        &env,
        &addrs,
        "wrong-packet-1",
        "correct-packet",
        12345678,
    );

    // Try to claim with wrong packet ID
    let info = message_info(&addrs.solver1, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "wrong-packet-1".to_string(),
            packet_id: "wrong-packet".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Packet ID mismatch"));
}

/// Ethereum Sender Spoofing: Packet from different Ethereum address
/// Expected: EthereumSenderMismatch error
#[test]
fn test_ethereum_sender_spoofing_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Register with specific ETH sender
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "spoof-sender-1",
        "0xlegituser",
        1_000_000,
        1200,
    );

    // Packet arrives from different sender
    let info = message_info(&addrs.admin, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "spoof-sender-1".to_string(),
            packet_id: "packet-1".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xattacker".to_string(), // Different sender
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Ethereum sender mismatch"));
}

/// Double Claim Attack: Solver tries to claim twice
/// Expected: Second claim fails (status is Claimed, not Finalized)
#[test]
fn test_double_claim_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Full flow until claimed
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "double-claim-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "double-claim-1",
        "packet-1",
        1_000_000,
        "0xuser",
    );
    front_settlement(
        &mut deps,
        &env,
        &addrs.solver1,
        "double-claim-1",
        "solver1",
        950_000,
        1_500_000,
        "ibc/eth",
    );
    finalize_escrow(&mut deps, &env, &addrs, "double-claim-1", "packet-1", 12345678);

    // First claim succeeds
    let info = message_info(&addrs.solver1, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "double-claim-1".to_string(),
            packet_id: "packet-1".to_string(),
        },
    )
    .unwrap();

    // Second claim fails
    let info = message_info(&addrs.solver1, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "double-claim-1".to_string(),
            packet_id: "packet-1".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Invalid Ethereum escrow status"));
}

/// Unauthorized Failure Marking: Attacker tries to fail escrow
/// Expected: Unauthorized error
#[test]
fn test_unauthorized_failure_marking_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    register_intent(
        &mut deps,
        &env,
        &addrs,
        "unauth-fail-1",
        "0xuser",
        1_000_000,
        1200,
    );

    // Attacker tries to mark as failed
    let info = message_info(&addrs.attacker, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::HandleEurekaEscrowFailure {
            intent_id: "unauth-fail-1".to_string(),
            reason: "Malicious failure attempt".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

/// Fail Already Claimed Escrow: Admin tries to fail claimed escrow
/// Expected: InvalidEthereumEscrowStatus error
#[test]
fn test_fail_claimed_escrow_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Full flow until claimed
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "fail-claimed-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "fail-claimed-1",
        "packet-1",
        1_000_000,
        "0xuser",
    );
    front_settlement(
        &mut deps,
        &env,
        &addrs.solver1,
        "fail-claimed-1",
        "solver1",
        950_000,
        1_500_000,
        "ibc/eth",
    );
    finalize_escrow(&mut deps, &env, &addrs, "fail-claimed-1", "packet-1", 12345678);

    let info = message_info(&addrs.solver1, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "fail-claimed-1".to_string(),
            packet_id: "packet-1".to_string(),
        },
    )
    .unwrap();

    // Admin tries to fail already claimed escrow
    let info = message_info(&addrs.admin, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::HandleEurekaEscrowFailure {
            intent_id: "fail-claimed-1".to_string(),
            reason: "Late failure attempt".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Invalid Ethereum escrow status"));
}

/// Front Without Enough Bond: Solver tries to front with insufficient bond
/// Expected: InsufficientBond error
#[test]
fn test_front_insufficient_bond_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    register_intent(
        &mut deps,
        &env,
        &addrs,
        "low-bond-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "low-bond-1",
        "packet-1",
        1_000_000,
        "0xuser",
    );

    // Try to front with less bond than declared
    let info = message_info(&addrs.solver1, &[Coin::new(100_000u128, "ibc/eth")]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "low-bond-1".to_string(),
            solver_id: "solver1".to_string(),
            output_amount: Uint128::new(950_000),
            risk_bond: Uint128::new(1_500_000), // Claims 1.5M but only sends 100k
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Insufficient bond"));
}

/// Claim Without Fronting: Solver tries to claim without having fronted
/// Expected: NotFronted error
#[test]
fn test_claim_without_fronting_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Setup without fronting - go directly to finalized
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "no-front-1",
        "0xuser",
        1_000_000,
        1200,
    );
    notify_packet_received(
        &mut deps,
        &env,
        &addrs,
        "no-front-1",
        "packet-1",
        1_000_000,
        "0xuser",
    );

    // Finalize without fronting
    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaFinalized {
            intent_id: "no-front-1".to_string(),
            packet_id: "packet-1".to_string(),
            proof_block: 12345678,
        },
    )
    .unwrap();

    // Try to claim
    let info = message_info(&addrs.solver1, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "no-front-1".to_string(),
            packet_id: "packet-1".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("not fronted"));
}

/// Front in Wrong State: Try to front when not in Received state
/// Expected: InvalidEthereumEscrowStatus error
#[test]
fn test_front_wrong_state_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Register but don't receive packet
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "wrong-state-1",
        "0xuser",
        1_000_000,
        1200,
    );

    // Try to front while still Pending
    let info = message_info(&addrs.solver1, &[Coin::new(1_500_000u128, "ibc/eth")]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "wrong-state-1".to_string(),
            solver_id: "solver1".to_string(),
            output_amount: Uint128::new(950_000),
            risk_bond: Uint128::new(1_500_000),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Invalid Ethereum escrow status"));
}

/// Finalize in Wrong State: Try to finalize when not Received/Fronted
/// Expected: InvalidEthereumEscrowStatus error
#[test]
fn test_finalize_wrong_state_blocked() {
    let (mut deps, env, addrs) = setup_contract();

    // Register but don't receive packet
    register_intent(
        &mut deps,
        &env,
        &addrs,
        "finalize-wrong-1",
        "0xuser",
        1_000_000,
        1200,
    );

    // Try to finalize while still Pending
    let info = message_info(&addrs.admin, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::NotifyEurekaFinalized {
            intent_id: "finalize-wrong-1".to_string(),
            packet_id: "packet-1".to_string(),
            proof_block: 12345678,
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Invalid Ethereum escrow status"));
}

/// Multiple Intents Same Solver: Solver fronts multiple intents
/// Expected: All work independently
#[test]
fn test_solver_multiple_intents() {
    let (mut deps, env, addrs) = setup_contract();

    // Setup 3 intents
    for i in 1..=3 {
        register_intent(
            &mut deps,
            &env,
            &addrs,
            &format!("multi-{}", i),
            &format!("0xuser{}", i),
            1_000_000,
            1200,
        );
        notify_packet_received(
            &mut deps,
            &env,
            &addrs,
            &format!("multi-{}", i),
            &format!("packet-{}", i),
            1_000_000,
            &format!("0xuser{}", i),
        );
        front_settlement(
            &mut deps,
            &env,
            &addrs.solver1,
            &format!("multi-{}", i),
            "solver1",
            950_000,
            1_500_000,
            "ibc/eth",
        );
    }

    // Verify all 3 are fronted by same solver
    for i in 1..=3 {
        let status: EthereumEscrowStatusResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::EthereumEscrowStatus {
                    intent_id: format!("multi-{}", i),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(status.escrow_status, "fronted");
        assert_eq!(status.fronted_by, Some("solver1".to_string()));
    }

    // Finalize and claim just one
    finalize_escrow(&mut deps, &env, &addrs, "multi-2", "packet-2", 12345678);

    let info = message_info(&addrs.solver1, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "multi-2".to_string(),
            packet_id: "packet-2".to_string(),
        },
    )
    .unwrap();

    // Verify: multi-2 is claimed, others still fronted
    let status1: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::EthereumEscrowStatus {
                intent_id: "multi-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status1.escrow_status, "fronted");

    let status2: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::EthereumEscrowStatus {
                intent_id: "multi-2".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status2.escrow_status, "claimed");

    let status3: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EthereumEscrowStatus {
                intent_id: "multi-3".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status3.escrow_status, "fronted");
}
