//! Integration tests for Ethereum escrow flow (Phase 2)
//!
//! These tests cover the complete Ethereum escrow lifecycle including:
//! - Intent creation and status transitions
//! - Solver fronting configuration and risk assessment
//! - Bond calculations with various risk pricing strategies
//! - Quote adjustments and failure handling

use atom_intents_solver::{EurekaSolver, SkipGoClient, Solver};
use atom_intents_types::{
    Asset, AssetPreference, EscrowStatus, EthereumEscrowedIntent, ExecutionConstraints, FillConfig,
    FrontingRiskAssessment, Intent, OutputSpec, SettlementPreference, SettlementRiskPricing,
};
use cosmwasm_std::{Binary, Uint128};
use std::sync::Arc;

/// Create a test Ethereum escrowed intent with default values
fn create_ethereum_escrowed_intent() -> EthereumEscrowedIntent {
    let base_intent = Intent {
        id: "eth-escrow-test-1".to_string(),
        version: "1".to_string(),
        nonce: 1,
        user: "cosmos1user".to_string(),
        input: Asset {
            chain_id: "ethereum-1".to_string(),
            denom: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(), // USDC on Ethereum
            amount: Uint128::new(1_000_000_000),                             // 1000 USDC
        },
        output: OutputSpec {
            chain_id: "cosmoshub-4".to_string(),
            denom: "uatom".to_string(),
            min_amount: Uint128::new(100_000_000), // 100 ATOM
            limit_price: "0.1".to_string(),
            recipient: "cosmos1recipient".to_string(),
        },
        fill_config: FillConfig::default(),
        constraints: ExecutionConstraints::new(9999999999)
            .with_asset_preference(AssetPreference::AnyEquivalent)
            .with_settlement_preference(SettlementPreference::Latency)
            .with_cross_ecosystem(true),
        signature: Binary::default(),
        public_key: Binary::default(),
        created_at: 1704067200,
        expires_at: 9999999999,
    };

    EthereumEscrowedIntent::new(
        base_intent,
        "0x1234567890abcdef".to_string(),
        300, // 5 min timeout
    )
}

// =============================================================================
// INTENT CREATION AND LIFECYCLE TESTS
// =============================================================================

#[test]
fn test_ethereum_escrow_intent_creation() {
    let escrowed = create_ethereum_escrowed_intent();

    assert_eq!(escrowed.eth_sender, "0x1234567890abcdef");
    assert_eq!(escrowed.eureka_timeout_secs, 300);
    assert!(escrowed.is_pending());
    assert!(!escrowed.is_ready_for_fronting());
}

#[test]
fn test_ethereum_escrow_lifecycle() {
    let mut escrowed = create_ethereum_escrowed_intent();

    // Phase 1: Pending
    assert!(escrowed.is_pending());
    assert!(!escrowed.is_ready_for_fronting());

    // Phase 2: Packet received
    escrowed.mark_received("pkt-123".to_string(), 1704067200);
    assert!(!escrowed.is_pending());
    assert!(escrowed.is_ready_for_fronting());

    // Phase 3: Finalized
    escrowed.mark_finalized("pkt-123".to_string(), 1704067230);
    assert!(!escrowed.is_ready_for_fronting());
    assert!(escrowed.is_finalized());
}

#[test]
fn test_escrow_status_transitions() {
    let mut escrowed = create_ethereum_escrowed_intent();

    // Verify initial status
    assert!(matches!(escrowed.escrow_status, EscrowStatus::Pending));

    // Transition to received
    escrowed.mark_received("pkt-abc".to_string(), 1704067200);
    match &escrowed.escrow_status {
        EscrowStatus::Received {
            packet_id,
            received_at,
        } => {
            assert_eq!(packet_id, "pkt-abc");
            assert_eq!(*received_at, 1704067200);
        }
        _ => panic!("Expected Received status"),
    }

    // Transition to finalized
    escrowed.mark_finalized("pkt-abc".to_string(), 1704067300);
    match &escrowed.escrow_status {
        EscrowStatus::Finalized {
            packet_id,
            finalized_at,
        } => {
            assert_eq!(packet_id, "pkt-abc");
            assert_eq!(*finalized_at, 1704067300);
        }
        _ => panic!("Expected Finalized status"),
    }
}

// =============================================================================
// SOLVER FRONTING SETUP TESTS
// =============================================================================

#[test]
fn test_eureka_solver_fronting_setup() {
    let solver =
        EurekaSolver::new("fronting-solver", Arc::new(SkipGoClient::mainnet())).with_fronting(true);

    assert!(solver.capabilities().cross_ecosystem);

    // Create an escrowed intent that's ready for fronting
    let mut escrowed = create_ethereum_escrowed_intent();
    escrowed.mark_received("pkt-456".to_string(), 1704067200);

    assert!(solver.can_front_settlement(&escrowed));
}

#[test]
fn test_eureka_solver_fronting_disabled() {
    let solver = EurekaSolver::new("no-fronting-solver", Arc::new(SkipGoClient::mainnet()));
    // Fronting disabled by default

    let mut escrowed = create_ethereum_escrowed_intent();
    escrowed.mark_received("pkt-789".to_string(), 1704067200);

    assert!(!solver.can_front_settlement(&escrowed));
}

#[test]
fn test_fronting_requires_received_status() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet())).with_fronting(true);

    // Pending status - should not be frontable
    let pending_escrowed = create_ethereum_escrowed_intent();
    assert!(!solver.can_front_settlement(&pending_escrowed));

    // Received status - should be frontable
    let mut received_escrowed = create_ethereum_escrowed_intent();
    received_escrowed.mark_received("pkt-123".to_string(), 1704067200);
    assert!(solver.can_front_settlement(&received_escrowed));

    // Finalized status - no need to front
    let mut finalized_escrowed = create_ethereum_escrowed_intent();
    finalized_escrowed.mark_finalized("pkt-123".to_string(), 1704067300);
    assert!(!solver.can_front_settlement(&finalized_escrowed));
}

// =============================================================================
// BOND CALCULATION TESTS
// =============================================================================

#[test]
fn test_fronting_bond_calculation() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Default 1.5x multiplier (realistic for Ethereum finality)
    let bond = solver.calculate_fronting_bond(Uint128::new(1_000_000));
    assert_eq!(bond, Uint128::new(1_500_000));

    // Test with larger amount
    let bond = solver.calculate_fronting_bond(Uint128::new(100_000_000));
    assert_eq!(bond, Uint128::new(150_000_000));
}

#[test]
fn test_fronting_bond_with_small_amounts() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Test with small amounts (1.5x multiplier)
    let bond = solver.calculate_fronting_bond(Uint128::new(1));
    assert_eq!(bond, Uint128::new(1)); // Truncated from 1.5

    let bond = solver.calculate_fronting_bond(Uint128::new(100));
    assert_eq!(bond, Uint128::new(150));
}

// =============================================================================
// RISK ASSESSMENT TESTS
// =============================================================================

#[test]
fn test_fronting_risk_assessment_profitable() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Profitable: solver receives 1M, pays user 900k, keeps 10%
    let assessment = solver.assess_fronting(
        Uint128::new(1_000_000), // fronted_amount (what solver gets)
        Uint128::new(900_000),   // solver_bid (what solver pays user)
    );
    assert!(assessment.should_front);
    assert!(assessment.expected_value > 0);
}

#[test]
fn test_fronting_risk_assessment_unprofitable() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Unprofitable: solver receives 1M but pays user 1.1M (loss)
    let assessment = solver.assess_fronting(Uint128::new(1_000_000), Uint128::new(1_100_000));
    assert!(!assessment.should_front);
}

#[test]
fn test_fronting_risk_assessment_marginal() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Marginal: solver receives 1M, pays 995k (0.5% profit)
    let assessment = solver.assess_fronting(Uint128::new(1_000_000), Uint128::new(995_000));

    // Thin margin - depends on risk calculation
    println!(
        "Marginal assessment: should_front={}, ev={}",
        assessment.should_front, assessment.expected_value
    );
}

#[test]
fn test_fronting_risk_assessment_struct_fields() {
    let pricing = SettlementRiskPricing::default_eureka();
    // fronted_amount=1M, solver_bid=950k (solver profits 50k)
    let assessment =
        FrontingRiskAssessment::assess(&pricing, Uint128::new(1_000_000), Uint128::new(950_000));

    // Verify all fields are populated
    assert!(assessment.required_bond > Uint128::zero());
    assert!(!assessment.reason.is_empty());
    assert!(assessment.should_front); // Should be profitable
}

// =============================================================================
// CUSTOM RISK PRICING TESTS
// =============================================================================

#[test]
fn test_custom_risk_pricing_conservative() {
    let conservative = SettlementRiskPricing::conservative();
    let solver = EurekaSolver::new("conservative", Arc::new(SkipGoClient::mainnet()))
        .with_fronting(true)
        .with_risk_pricing(conservative);

    // Conservative pricing: 2x multiplier (vs 1.5x default)
    let bond = solver.calculate_fronting_bond(Uint128::new(1_000_000));
    assert_eq!(bond, Uint128::new(2_000_000));
}

#[test]
fn test_default_eureka_pricing() {
    let pricing = SettlementRiskPricing::default_eureka();

    // Realistic Ethereum finality: ~20 minutes
    assert_eq!(pricing.bond_multiplier, "1.5");
    assert_eq!(pricing.expected_finality_secs, 1200); // 20 minutes
    assert_eq!(pricing.failure_probability, "0.0001");
}

#[test]
fn test_source_specific_pricing() {
    // OP Stack L2s (Base, Optimism, etc.)
    let op_stack = SettlementRiskPricing::default_op_stack_l2();
    assert_eq!(op_stack.expected_finality_secs, 1200); // L1 batch finality
    assert_eq!(op_stack.failure_probability, "0.0005");

    // ZK rollups
    let zk = SettlementRiskPricing::default_zk_rollup();
    assert_eq!(zk.expected_finality_secs, 900); // 15 min
    assert_eq!(zk.failure_probability, "0.0002");
}

#[test]
fn test_conservative_pricing_parameters() {
    let pricing = SettlementRiskPricing::conservative();

    assert_eq!(pricing.bond_multiplier, "2.0");
    assert_eq!(pricing.expected_finality_secs, 1500); // 25 minutes
    assert_eq!(pricing.failure_probability, "0.005");
}

#[test]
fn test_risk_pricing_bond_calculation() {
    let default_pricing = SettlementRiskPricing::default_eureka();
    let conservative_pricing = SettlementRiskPricing::conservative();

    let amount = Uint128::new(1_000_000);

    let default_bond = default_pricing.required_bond(amount);
    let conservative_bond = conservative_pricing.required_bond(amount);

    assert_eq!(default_bond, Uint128::new(1_500_000)); // 1.5x
    assert_eq!(conservative_bond, Uint128::new(2_000_000)); // 2x
}

// =============================================================================
// FAILURE HANDLING TESTS
// =============================================================================

#[test]
fn test_escrow_failure_handling() {
    let mut escrowed = create_ethereum_escrowed_intent();

    // Simulate packet failure
    escrowed.mark_failed("timeout: no packet received".to_string());

    assert!(escrowed.is_failed());
    assert!(!escrowed.is_pending());
    assert!(!escrowed.is_ready_for_fronting());
}

#[test]
fn test_escrow_failure_with_reason() {
    let mut escrowed = create_ethereum_escrowed_intent();

    escrowed.mark_failed("insufficient funds on Ethereum".to_string());

    match &escrowed.escrow_status {
        EscrowStatus::Failed { reason } => {
            assert_eq!(reason, "insufficient funds on Ethereum");
        }
        _ => panic!("Expected Failed status"),
    }
}

#[test]
fn test_failed_escrow_not_frontable() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet())).with_fronting(true);

    let mut escrowed = create_ethereum_escrowed_intent();
    escrowed.mark_failed("packet timeout".to_string());

    assert!(!solver.can_front_settlement(&escrowed));
}

// =============================================================================
// AUCTION-DISCOVERED PRICING TESTS
// =============================================================================

#[test]
fn test_solver_bid_determines_profit() {
    // Risk premium is discovered through the auction - solvers bid their total spread
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Solver receives 1M from escrow
    let fronted_amount = Uint128::new(1_000_000);

    // If solver bids to pay user 950k, they profit 50k (5% spread)
    let assessment = solver.assess_fronting(fronted_amount, Uint128::new(950_000));
    assert!(assessment.should_front);
    assert!(assessment.expected_value > 0);

    // If solver bids to pay user 1M, they make no profit
    let assessment = solver.assess_fronting(fronted_amount, Uint128::new(1_000_000));
    assert!(!assessment.should_front);
}

#[test]
fn test_bond_requirement_independent_of_spread() {
    // Bond is protocol-enforced, not dependent on solver's bid
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    let fronted = Uint128::new(1_000_000);
    let bond = solver.calculate_fronting_bond(fronted);

    // Bond is always 1.5x regardless of what solver bids
    assert_eq!(bond, Uint128::new(1_500_000));
}

// =============================================================================
// CAPACITY AND ASYNC TESTS
// =============================================================================

#[tokio::test]
async fn test_eureka_solver_capacity_with_fronting() {
    let solver =
        EurekaSolver::new("capacity-test", Arc::new(SkipGoClient::mainnet())).with_fronting(true);

    let pair = solver.supported_pairs().first().unwrap().clone();
    let capacity = solver.capacity(&pair).await.unwrap();

    // Eureka has deep liquidity
    assert!(capacity.max_immediate > Uint128::zero());
    assert!(capacity.estimated_time_ms >= 20_000); // At least 20s for Eureka
}

#[tokio::test]
async fn test_eureka_solver_health_check_with_fronting() {
    let solver =
        EurekaSolver::new("health-test", Arc::new(SkipGoClient::mainnet())).with_fronting(true);

    assert!(solver.health_check().await);
}

// =============================================================================
// ESCROW STATUS HELPER METHOD TESTS
// =============================================================================

#[test]
fn test_escrow_status_packet_id() {
    // Pending has no packet_id
    let pending = EscrowStatus::Pending;
    assert!(pending.packet_id().is_none());

    // Received has packet_id
    let received = EscrowStatus::Received {
        packet_id: "pkt-123".to_string(),
        received_at: 1704067200,
    };
    assert_eq!(received.packet_id(), Some("pkt-123"));

    // Finalized has packet_id
    let finalized = EscrowStatus::Finalized {
        packet_id: "pkt-456".to_string(),
        finalized_at: 1704067300,
    };
    assert_eq!(finalized.packet_id(), Some("pkt-456"));

    // Failed has no packet_id
    let failed = EscrowStatus::Failed {
        reason: "timeout".to_string(),
    };
    assert!(failed.packet_id().is_none());
}

#[test]
fn test_escrow_status_helper_methods() {
    let pending = EscrowStatus::Pending;
    assert!(!pending.is_finalized());
    assert!(!pending.is_failed());
    assert!(!pending.is_received());

    let received = EscrowStatus::Received {
        packet_id: "pkt".to_string(),
        received_at: 0,
    };
    assert!(!received.is_finalized());
    assert!(!received.is_failed());
    assert!(received.is_received());

    let finalized = EscrowStatus::Finalized {
        packet_id: "pkt".to_string(),
        finalized_at: 0,
    };
    assert!(finalized.is_finalized());
    assert!(!finalized.is_failed());
    assert!(!finalized.is_received());

    let failed = EscrowStatus::Failed {
        reason: "err".to_string(),
    };
    assert!(!failed.is_finalized());
    assert!(failed.is_failed());
    assert!(!failed.is_received());
}

// =============================================================================
// INTEGRATION SCENARIO TESTS
// =============================================================================

#[test]
fn test_complete_fronting_flow() {
    // Simulate a complete fronting scenario

    // 1. Create solver with fronting enabled
    let solver =
        EurekaSolver::new("fronting-solver", Arc::new(SkipGoClient::mainnet())).with_fronting(true);

    // 2. Create an escrowed intent
    let mut escrowed = create_ethereum_escrowed_intent();

    // 3. Initially, cannot front (pending)
    assert!(!solver.can_front_settlement(&escrowed));

    // 4. Packet arrives - mark as received
    escrowed.mark_received("eureka-pkt-001".to_string(), 1704067200);

    // 5. Now can front
    assert!(solver.can_front_settlement(&escrowed));

    // 6. Calculate required bond (1.5x multiplier)
    let input_amount = escrowed.base.input.amount;
    let bond = solver.calculate_fronting_bond(input_amount);
    // 1.5x bond: 1_000_000_000 * 1.5 = 1_500_000_000
    assert_eq!(bond, Uint128::new(1_500_000_000));

    // 7. Assess risk for a profitable trade
    // Solver receives input_amount from escrow, bids to pay user less (their spread)
    let solver_bid = Uint128::new(950_000_000); // Solver keeps 5% as spread
    let assessment = solver.assess_fronting(input_amount, solver_bid);
    assert!(assessment.should_front);
    assert!(assessment.expected_value > 0);

    // 8. After ZK proof finalizes
    escrowed.mark_finalized("eureka-pkt-001".to_string(), 1704067230);

    // 9. No longer needs fronting
    assert!(!solver.can_front_settlement(&escrowed));
    assert!(escrowed.is_finalized());
}

#[test]
fn test_fronting_decision_based_on_profit_margin() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Solver receives 1M from escrow
    let fronted = Uint128::new(1_000_000);

    // High profit margin - solver pays user 800k, keeps 200k (20% profit)
    let high_profit = solver.assess_fronting(fronted, Uint128::new(800_000));
    assert!(high_profit.should_front);

    // Medium profit margin - solver pays user 950k, keeps 50k (5% profit)
    let medium_profit = solver.assess_fronting(fronted, Uint128::new(950_000));
    assert!(medium_profit.should_front);

    // No profit margin - solver pays user exactly what they receive
    let no_profit = solver.assess_fronting(fronted, Uint128::new(1_000_000));
    assert!(!no_profit.should_front);

    // Negative margin - solver pays more than they receive (loss)
    let negative = solver.assess_fronting(fronted, Uint128::new(1_100_000));
    assert!(!negative.should_front);
}
