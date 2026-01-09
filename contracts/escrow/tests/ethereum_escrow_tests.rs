//! Tests for Ethereum escrow handlers (via Eureka)

use atom_intents_escrow::contract::{execute, instantiate, query};
use atom_intents_escrow::msg::{
    EthereumEscrowStatusResponse, ExecuteMsg, InstantiateMsg, QueryMsg,
};
use atom_intents_types::{Asset, ExecutionConstraints, FillConfig, Intent, OutputSpec};
use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
use cosmwasm_std::{from_json, Addr, Binary, Coin, Timestamp, Uint128};

// Helper to get test addresses
struct TestAddrs {
    admin: Addr,
    settlement: Addr,
    solver: Addr,
    user: Addr,
}

fn test_addrs(api: &MockApi) -> TestAddrs {
    TestAddrs {
        admin: api.addr_make("admin"),
        settlement: api.addr_make("settlement"),
        solver: api.addr_make("solver"),
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

/// Create a test intent for Ethereum escrow
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

// ═══════════════════════════════════════════════════════════════════════════════
// REGISTER ETHEREUM ESCROW INTENT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_register_ethereum_escrow_intent_success() {
    let (mut deps, env, addrs) = setup_contract();

    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent.clone()),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    assert_eq!(res.attributes[0].value, "register_ethereum_escrow_intent");
    assert_eq!(res.attributes[1].value, "intent-eth-1");
    assert_eq!(res.attributes[2].value, "0xuser123");

    // Query status
    let status: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EthereumEscrowStatus {
                intent_id: "intent-eth-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(status.intent_id, "intent-eth-1");
    assert_eq!(status.eth_sender, "0xuser123");
    assert_eq!(status.expected_amount, Uint128::new(1_000_000));
    assert_eq!(status.escrow_status, "pending");
    assert!(status.packet_id.is_none());
    assert!(status.fronted_by.is_none());
}

#[test]
fn test_register_ethereum_escrow_intent_unauthorized() {
    let (mut deps, env, addrs) = setup_contract();

    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.user, &[]); // Unauthorized user

    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn test_register_ethereum_escrow_intent_duplicate_fails() {
    let (mut deps, env, addrs) = setup_contract();

    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    // First registration succeeds
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent.clone()),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    // Second registration fails
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Intent already has escrow"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// NOTIFY EUREKA PACKET RECEIVED TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_notify_eureka_packet_received_success() {
    let (mut deps, env, addrs) = setup_contract();

    // Register intent first
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    // Notify packet received
    let info = message_info(&addrs.admin, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap();

    assert_eq!(res.attributes[0].value, "notify_eureka_packet_received");

    // Query status
    let status: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EthereumEscrowStatus {
                intent_id: "intent-eth-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(status.escrow_status, "received");
    assert_eq!(status.packet_id, Some("packet-123".to_string()));
}

#[test]
fn test_notify_eureka_packet_received_wrong_sender_fails() {
    let (mut deps, env, addrs) = setup_contract();

    // Register intent
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    // Notify with wrong sender
    let info = message_info(&addrs.admin, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xwrong".to_string(), // Wrong sender
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Ethereum sender mismatch"));
}

#[test]
fn test_notify_eureka_packet_received_timeout_fails() {
    let (mut deps, mut env, addrs) = setup_contract();

    // Register intent
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    // Fast forward past timeout
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 2000);

    // Notify should fail due to timeout
    let info = message_info(&addrs.admin, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Eureka timeout"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// NOTIFY EUREKA FINALIZED TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_notify_eureka_finalized_success() {
    let (mut deps, env, addrs) = setup_contract();

    // Register and receive packet
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap();

    // Notify finalized
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaFinalized {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            proof_block: 12345678,
        },
    )
    .unwrap();

    assert_eq!(res.attributes[0].value, "notify_eureka_finalized");
    assert_eq!(res.attributes[2].value, "12345678"); // proof_block

    // Query status
    let status: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EthereumEscrowStatus {
                intent_id: "intent-eth-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(status.escrow_status, "finalized");
}

// ═══════════════════════════════════════════════════════════════════════════════
// FRONT SETTLEMENT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_front_settlement_success() {
    let (mut deps, env, addrs) = setup_contract();

    // Register and receive packet
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap();

    // Solver fronts settlement with bond
    let info = message_info(&addrs.solver, &[Coin::new(1_500_000u128, "ibc/eth")]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "intent-eth-1".to_string(),
            solver_id: "solver-1".to_string(),
            output_amount: Uint128::new(950_000),
            risk_bond: Uint128::new(1_500_000),
        },
    )
    .unwrap();

    assert_eq!(res.attributes[0].value, "front_settlement");
    assert_eq!(res.attributes[2].value, "solver-1");

    // Query status
    let status: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EthereumEscrowStatus {
                intent_id: "intent-eth-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(status.escrow_status, "fronted");
    assert_eq!(status.fronted_by, Some("solver-1".to_string()));
}

#[test]
fn test_front_settlement_insufficient_bond_fails() {
    let (mut deps, env, addrs) = setup_contract();

    // Register and receive packet
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap();

    // Solver fronts with insufficient bond
    let info = message_info(&addrs.solver, &[Coin::new(500_000u128, "ibc/eth")]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "intent-eth-1".to_string(),
            solver_id: "solver-1".to_string(),
            output_amount: Uint128::new(950_000),
            risk_bond: Uint128::new(1_500_000), // Requires 1.5M but only provided 500k
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Insufficient bond"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// CLAIM EUREKA ESCROW TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_claim_eureka_escrow_success() {
    let (mut deps, env, addrs) = setup_contract();

    // Full flow: register -> receive -> front -> finalize -> claim
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap();

    // Solver fronts
    let info = message_info(&addrs.solver, &[Coin::new(1_500_000u128, "ibc/eth")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "intent-eth-1".to_string(),
            solver_id: "solver-1".to_string(),
            output_amount: Uint128::new(950_000),
            risk_bond: Uint128::new(1_500_000),
        },
    )
    .unwrap();

    // Finalize
    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaFinalized {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            proof_block: 12345678,
        },
    )
    .unwrap();

    // Solver claims
    let info = message_info(&addrs.solver, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
        },
    )
    .unwrap();

    assert_eq!(res.attributes[0].value, "claim_eureka_escrow");
    // Should have a bank message to send funds to solver
    assert_eq!(res.messages.len(), 1);

    // Query status
    let status: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EthereumEscrowStatus {
                intent_id: "intent-eth-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(status.escrow_status, "claimed");
}

#[test]
fn test_claim_eureka_escrow_not_finalized_fails() {
    let (mut deps, env, addrs) = setup_contract();

    // Register -> receive -> front (no finalize)
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap();

    // Solver fronts
    let info = message_info(&addrs.solver, &[Coin::new(1_500_000u128, "ibc/eth")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "intent-eth-1".to_string(),
            solver_id: "solver-1".to_string(),
            output_amount: Uint128::new(950_000),
            risk_bond: Uint128::new(1_500_000),
        },
    )
    .unwrap();

    // Try to claim without finalization
    let info = message_info(&addrs.solver, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Invalid Ethereum escrow status"));
}

#[test]
fn test_claim_eureka_escrow_wrong_solver_fails() {
    let (mut deps, env, addrs) = setup_contract();

    // Full flow: register -> receive -> front -> finalize
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap();

    let info = message_info(&addrs.solver, &[Coin::new(1_500_000u128, "ibc/eth")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "intent-eth-1".to_string(),
            solver_id: "solver-1".to_string(),
            output_amount: Uint128::new(950_000),
            risk_bond: Uint128::new(1_500_000),
        },
    )
    .unwrap();

    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaFinalized {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            proof_block: 12345678,
        },
    )
    .unwrap();

    // Different user tries to claim
    let info = message_info(&addrs.user, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// HANDLE EUREKA ESCROW FAILURE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_handle_eureka_escrow_failure_success() {
    let (mut deps, env, addrs) = setup_contract();

    // Register intent
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    // Admin marks as failed
    let info = message_info(&addrs.admin, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::HandleEurekaEscrowFailure {
            intent_id: "intent-eth-1".to_string(),
            reason: "Eureka packet timed out".to_string(),
        },
    )
    .unwrap();

    assert_eq!(res.attributes[0].value, "handle_eureka_escrow_failure");
    assert_eq!(res.attributes[2].value, "Eureka packet timed out");

    // Query status
    let status: EthereumEscrowStatusResponse = from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::EthereumEscrowStatus {
                intent_id: "intent-eth-1".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert!(status.escrow_status.starts_with("failed:"));
}

#[test]
fn test_handle_eureka_escrow_failure_returns_bond() {
    let (mut deps, env, addrs) = setup_contract();

    // Full flow until fronted
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    let info = message_info(&addrs.admin, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id: "intent-eth-1".to_string(),
            packet_id: "packet-123".to_string(),
            amount: Uint128::new(1_000_000),
            sender: "0xuser123".to_string(),
        },
    )
    .unwrap();

    // Solver fronts
    let info = message_info(&addrs.solver, &[Coin::new(1_500_000u128, "ibc/eth")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::FrontSettlement {
            intent_id: "intent-eth-1".to_string(),
            solver_id: "solver-1".to_string(),
            output_amount: Uint128::new(950_000),
            risk_bond: Uint128::new(1_500_000),
        },
    )
    .unwrap();

    // Admin marks as failed - bond should be returned
    let info = message_info(&addrs.admin, &[]);
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::HandleEurekaEscrowFailure {
            intent_id: "intent-eth-1".to_string(),
            reason: "ZK proof failed verification".to_string(),
        },
    )
    .unwrap();

    assert_eq!(res.attributes[3].value, "returned"); // bond_action
    assert_eq!(res.messages.len(), 1); // Bond return message
}

#[test]
fn test_handle_eureka_escrow_failure_unauthorized() {
    let (mut deps, env, addrs) = setup_contract();

    // Register intent
    let intent = create_test_intent("intent-eth-1", "ibc/eth", 1_000_000);
    let info = message_info(&addrs.settlement, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent: Box::new(intent),
            eth_sender: "0xuser123".to_string(),
            expected_amount: Uint128::new(1_000_000),
            eureka_timeout_secs: 1200,
        },
    )
    .unwrap();

    // Non-admin tries to mark as failed
    let info = message_info(&addrs.user, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::HandleEurekaEscrowFailure {
            intent_id: "intent-eth-1".to_string(),
            reason: "Attacker trying to fail escrow".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}
