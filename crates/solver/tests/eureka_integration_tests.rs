//! Integration tests for Eureka solver

use atom_intents_solver::{EurekaSolver, SkipGoClient, Solver};
use atom_intents_types::{
    Asset, AssetPreference, ExecutionConstraints, FillConfig, Intent, OutputSpec, SolveContext,
    TradingPair,
};
use cosmwasm_std::{Binary, Uint128};
use std::sync::Arc;

fn create_test_intent(
    asset_pref: AssetPreference,
    allow_cross_ecosystem: bool,
) -> Intent {
    Intent {
        id: "integration-test-1".to_string(),
        version: "1".to_string(),
        nonce: 1,
        user: "cosmos1testuser".to_string(),
        input: Asset {
            chain_id: "cosmoshub-4".to_string(),
            denom: "uatom".to_string(),
            amount: Uint128::new(100_000_000),
        },
        output: OutputSpec {
            chain_id: "noble-1".to_string(),
            denom: "uusdc".to_string(),
            min_amount: Uint128::new(500_000_000),
            limit_price: "5.0".to_string(),
            recipient: "cosmos1recipient".to_string(),
        },
        fill_config: FillConfig::default(),
        constraints: ExecutionConstraints::new(9999999999)
            .with_asset_preference(asset_pref)
            .with_cross_ecosystem(allow_cross_ecosystem),
        signature: Binary::default(),
        public_key: Binary::default(),
        created_at: 0,
        expires_at: 9999999999,
    }
}

#[test]
fn test_eureka_solver_setup() {
    let solver = EurekaSolver::new("integration-test-solver", Arc::new(SkipGoClient::mainnet()));
    assert_eq!(solver.id(), "integration-test-solver");
    assert!(solver.capabilities().cross_ecosystem);
    assert!(!solver.supported_pairs().is_empty());
}

#[test]
fn test_eureka_solver_constraint_validation() {
    // NativeOnly should not allow bridged delivery
    let native_only_constraints = ExecutionConstraints::default();
    assert!(!EurekaSolver::can_deliver_bridged(&native_only_constraints));

    // AcceptBridged should allow bridged delivery
    let bridged_constraints =
        ExecutionConstraints::new(1000).with_asset_preference(AssetPreference::AcceptBridged {
            allowed_denoms: vec!["eureka/usdc".to_string()],
        });
    assert!(EurekaSolver::can_deliver_bridged(&bridged_constraints));

    // AnyEquivalent should allow bridged delivery
    let any_constraints =
        ExecutionConstraints::new(1000).with_asset_preference(AssetPreference::AnyEquivalent);
    assert!(EurekaSolver::can_deliver_bridged(&any_constraints));
}

#[test]
fn test_eureka_solver_time_constraint_validation() {
    // No constraint - should be acceptable
    let no_constraint = ExecutionConstraints::default();
    assert!(EurekaSolver::is_time_acceptable(&no_constraint));

    // 30 seconds - should be acceptable (>= 25)
    let thirty_secs = ExecutionConstraints::new(1000).with_max_settlement_secs(30);
    assert!(EurekaSolver::is_time_acceptable(&thirty_secs));

    // 25 seconds - should be acceptable (== 25)
    let twenty_five = ExecutionConstraints::new(1000).with_max_settlement_secs(25);
    assert!(EurekaSolver::is_time_acceptable(&twenty_five));

    // 20 seconds - should NOT be acceptable (< 25)
    let twenty_secs = ExecutionConstraints::new(1000).with_max_settlement_secs(20);
    assert!(!EurekaSolver::is_time_acceptable(&twenty_secs));
}

#[tokio::test]
async fn test_eureka_solver_capacity() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));
    let pair = solver.supported_pairs().first().unwrap().clone();
    let capacity = solver.capacity(&pair).await.unwrap();
    assert!(capacity.max_immediate > Uint128::zero());
    assert!(capacity.estimated_time_ms >= 20_000);
}

#[tokio::test]
async fn test_eureka_solver_capacity_unsupported_pair() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));
    let unsupported_pair = TradingPair::new("uatom", "uosmo"); // Not an Eureka pair
    let result = solver.capacity(&unsupported_pair).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_eureka_solver_rejects_native_only() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Create intent with NativeOnly preference but cross-ecosystem allowed
    let intent = create_test_intent(AssetPreference::NativeOnly, true);

    let ctx = SolveContext {
        matched_amount: Uint128::zero(),
        remaining: intent.input.amount,
        oracle_price: "10.0".to_string(),
    };

    let result = solver.solve(&intent, &ctx).await;
    assert!(result.is_err());

    if let Err(e) = result {
        let error_str = format!("{:?}", e);
        assert!(error_str.contains("native-only"));
    }
}

#[tokio::test]
async fn test_eureka_solver_rejects_cross_ecosystem_disabled() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    // Create intent with cross-ecosystem disabled (default is false)
    let intent = create_test_intent(AssetPreference::AnyEquivalent, false);

    let ctx = SolveContext {
        matched_amount: Uint128::zero(),
        remaining: intent.input.amount,
        oracle_price: "10.0".to_string(),
    };

    let result = solver.solve(&intent, &ctx).await;
    assert!(result.is_err());

    if let Err(e) = result {
        let error_str = format!("{:?}", e);
        assert!(error_str.contains("cross-ecosystem"));
    }
}

#[tokio::test]
async fn test_eureka_solver_health_check() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));
    assert!(solver.health_check().await);
}

#[test]
fn test_supported_pairs() {
    let solver = EurekaSolver::new("test", Arc::new(SkipGoClient::mainnet()));

    let pairs = solver.supported_pairs();
    assert!(!pairs.is_empty());

    // Should have Ethereum USDC to Cosmos USDC
    let eth_usdc_pair =
        TradingPair::new("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "uusdc");
    assert!(pairs.contains(&eth_usdc_pair));
}

#[tokio::test]
#[ignore] // Requires network access to Skip Go API
async fn test_eureka_solver_live_quote() {
    let solver = EurekaSolver::new("live-test", Arc::new(SkipGoClient::mainnet()));

    let intent = create_test_intent(
        AssetPreference::AcceptBridged {
            allowed_denoms: vec!["eureka/usdc".to_string()],
        },
        true, // allow cross-ecosystem
    );

    let ctx = SolveContext {
        matched_amount: Uint128::zero(),
        remaining: intent.input.amount,
        oracle_price: "10.0".to_string(),
    };

    match solver.solve(&intent, &ctx).await {
        Ok(solution) => {
            println!("Got solution: {:?}", solution);
            assert_eq!(solution.solver_id, "live-test");
            assert!(solution.fill.output_amount > Uint128::zero());
            assert!(solution.metadata.is_some());
        }
        Err(e) => {
            println!("Solve error (may be expected): {:?}", e);
        }
    }
}
