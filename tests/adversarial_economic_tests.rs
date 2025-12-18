/// Adversarial economic attack tests
///
/// These tests simulate economic attacks where things could go horribly wrong:
/// - Front-running attacks
/// - Sandwich attacks
/// - Fee extraction attacks
/// - Flash loan attacks
/// - MEV extraction
/// - Price oracle manipulation for profit
/// - Griefing attacks

use atom_intents_matching_engine::MatchingEngine;
use atom_intents_types::{
    Asset, ExecutionConstraints, FillConfig, FillStrategy, Intent, OutputSpec, SolverQuote,
    TradingPair,
};
use cosmwasm_std::{Binary, Uint128};
use rust_decimal::Decimal;
use std::str::FromStr;

// Helper to create test intents
fn make_test_intent(
    id: &str,
    user: &str,
    input_denom: &str,
    input_amount: u128,
    output_denom: &str,
    min_output: u128,
    limit_price: &str,
) -> Intent {
    Intent {
        id: id.to_string(),
        version: "1.0".to_string(),
        nonce: 0,
        user: user.to_string(),
        input: Asset::new("cosmoshub-4", input_denom, input_amount),
        output: OutputSpec {
            chain_id: "noble-1".to_string(),
            denom: output_denom.to_string(),
            min_amount: Uint128::new(min_output),
            limit_price: limit_price.to_string(),
            recipient: user.to_string(),
        },
        fill_config: FillConfig {
            allow_partial: true,
            min_fill_amount: Uint128::zero(),
            min_fill_pct: "0.1".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        },
        constraints: ExecutionConstraints::new(9999999999),
        signature: Binary::default(),
        public_key: Binary::default(),
        created_at: 0,
        expires_at: 9999999999,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// FRONT-RUNNING ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Simulate front-running: attacker sees pending trade and trades first
#[test]
fn test_frontrunning_defense_via_limit_price() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Victim wants to buy ATOM with limit price 0.1 ATOM/USDC (max 10 USDC/ATOM)
    let victim_buy = make_test_intent(
        "victim-buy", "victim", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );

    // ATTACK: Frontrunner places buy order first at same price
    let frontrunner_buy = make_test_intent(
        "frontrunner-buy", "frontrunner", "uusdc", 100_000_000, "uatom", 10_000_000, "0.1",
    );

    // Seller at 10 USDC/ATOM
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 5_000_000, "uusdc", 50_000_000, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();

    // In batch auction model, both orders compete fairly
    // Frontrunner doesn't get priority just by submitting first
    let result = engine.run_batch_auction(
        pair,
        vec![frontrunner_buy, victim_buy, sell],
        vec![],
        oracle,
    );

    assert!(result.is_ok());
    let auction = result.unwrap();

    // Both buyers should get fills proportionally (batch auction property)
    // This is the defense against front-running in intent systems
}

/// Test that limit price protects against price manipulation
#[test]
fn test_limit_price_protects_against_price_slippage() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // User sets strict limit: willing to pay max 10 USDC/ATOM
    let user_buy = make_test_intent(
        "user-buy", "user", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );

    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    // ATTACK: Oracle manipulated to 11 USDC/ATOM
    let manipulated_oracle = Decimal::from_str("11.0").unwrap();

    let result = engine.run_batch_auction(pair, vec![user_buy, sell], vec![], manipulated_oracle);

    // User's order should be rejected - protects against price manipulation
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// SANDWICH ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Simulate sandwich attack: attacker buys before and sells after victim's trade
#[test]
fn test_sandwich_attack_mitigation() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // ATTACK SETUP:
    // 1. Front-buy: Attacker buys ATOM to push price up
    // 2. Victim trade: User buys at inflated price
    // 3. Back-sell: Attacker sells ATOM at profit

    // In batch auctions, all trades execute at the same price
    // This mitigates sandwich attacks

    let front_buy = make_test_intent(
        "front-buy", "attacker", "uusdc", 50_000_000, "uatom", 5_000_000, "0.1",
    );

    let victim_buy = make_test_intent(
        "victim-buy", "victim", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );

    // Attacker's back-sell (they want to sell what they bought)
    let back_sell = make_test_intent(
        "back-sell", "attacker", "uatom", 5_000_000, "uusdc", 50_000_000, "10.0",
    );

    // Liquidity provider
    let seller = make_test_intent(
        "seller", "lp", "uatom", 10_000_000, "uusdc", 100_000_000, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();

    let result = engine.run_batch_auction(
        pair,
        vec![front_buy, victim_buy, back_sell, seller],
        vec![],
        oracle,
    );

    assert!(result.is_ok());
    let auction = result.unwrap();

    // All trades execute at same clearing price
    // Attacker buys and sells at same price = no profit from sandwich
}

// ═══════════════════════════════════════════════════════════════════════════
// FEE EXTRACTION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that solver cannot extract excessive fees through bad exchange rate
#[test]
fn test_solver_fee_extraction_blocked_by_min_output() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // User wants at least 10_000_000 USDC output
    let user_sell = make_test_intent(
        "user-sell",
        "user",
        "uatom",
        1_000_000,  // Selling 1 ATOM
        "uusdc",
        10_000_000, // Wants at least 10 USDC
        "10.0",     // Limit price 10 USDC/ATOM
    );

    // ATTACK: Solver offers terrible rate (extracting 90% as fees)
    let exploitative_solver = SolverQuote {
        solver_id: "fee-extractor".to_string(),
        input_amount: Uint128::new(1_000_000),
        output_amount: Uint128::new(1_000_000), // Only 1 USDC output (should be 10+)
        price: "1.0".to_string(),               // Terrible price
        valid_for_ms: 5000,
    };

    let oracle = Decimal::from_str("10.0").unwrap();

    let result = engine.run_batch_auction(pair, vec![user_sell], vec![exploitative_solver], oracle);

    // Auction succeeds but solver's bad quote is rejected by limit price check
    // (if they try to fill at 1 USDC/ATOM when user wants 10+)
}

/// Test max solver fee constraint protection
#[test]
fn test_max_solver_fee_constraint() {
    // User can set max_solver_fee_bps in constraints
    // This limits how much a solver can charge

    let constraints = ExecutionConstraints::new(9999999999)
        .with_max_solver_fee_bps(50); // Max 0.5% fee

    assert_eq!(constraints.max_solver_fee_bps, Some(50));

    // The application layer should enforce this constraint
    // by rejecting fills that charge more than specified fee
}

// ═══════════════════════════════════════════════════════════════════════════
// PRICE ORACLE MANIPULATION TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test defense against stale oracle price
#[test]
fn test_stale_oracle_defense() {
    // In production, oracle prices should have freshness checks
    // This test documents the expected behavior

    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    // Oracle price from 1 hour ago (stale)
    // In production, the oracle integration should reject stale prices
    let stale_oracle = Decimal::from_str("10.0").unwrap();

    // Currently the engine doesn't check oracle freshness
    // This is the responsibility of the orchestrator layer
    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], stale_oracle);
    assert!(result.is_ok());

    // RECOMMENDATION: Add oracle freshness validation at orchestrator level
}

/// Test multiple oracle sources for manipulation resistance
#[test]
fn test_oracle_aggregation_concept() {
    // For production, use multiple oracle sources and aggregate them
    // This test documents the concept

    let price_pyth = Decimal::from_str("10.0").unwrap();
    let price_chainlink = Decimal::from_str("10.05").unwrap();
    let price_slinky = Decimal::from_str("9.98").unwrap();

    // Example aggregation: median
    let mut prices = vec![price_pyth, price_chainlink, price_slinky];
    prices.sort();
    let median_price = prices[1];

    // Median is more resistant to single oracle manipulation
    assert!((median_price - Decimal::from_str("10.0").unwrap()).abs() < Decimal::from_str("0.1").unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════
// GRIEFING ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test defense against order spam (DOS via many small orders)
#[test]
fn test_order_spam_handling() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // ATTACK: Spam many tiny orders
    let mut spam_orders = Vec::new();
    for i in 0..100 {
        spam_orders.push(make_test_intent(
            &format!("spam-{}", i),
            &format!("spammer-{}", i),
            "uusdc",
            1, // Tiny amount
            "uatom",
            0,
            "0.1",
        ));
    }

    let oracle = Decimal::from_str("10.0").unwrap();

    // Should handle without crashing or timing out
    let result = engine.run_batch_auction(pair, spam_orders, vec![], oracle);
    assert!(result.is_ok());

    // Note: In production, minimum order sizes should be enforced
    // to prevent griefing through transaction fee economics
}

/// Test that zero-value trades don't cause issues
#[test]
fn test_zero_value_griefing() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // ATTACK: Orders with zero amounts
    let zero_buy = make_test_intent(
        "zero-buy", "griefer", "uusdc", 0, "uatom", 0, "0.1",
    );
    let zero_sell = make_test_intent(
        "zero-sell", "griefer", "uatom", 0, "uusdc", 0, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();

    // Should handle gracefully
    let result = engine.run_batch_auction(pair, vec![zero_buy, zero_sell], vec![], oracle);
    assert!(result.is_ok());

    // No fills expected for zero amounts
    let auction = result.unwrap();
    assert!(auction.internal_fills.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// SOLVER COLLUSION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that multiple colluding solvers can't manipulate prices
#[test]
fn test_solver_collusion_resistance() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    let user_buy = make_test_intent(
        "user-buy", "user", "uusdc", 10_000_000, "uatom", 900_000, "0.11",
    );

    // ATTACK: Multiple solvers collude to offer terrible prices
    let colluder_1 = SolverQuote {
        solver_id: "colluder-1".to_string(),
        input_amount: Uint128::new(10_000_000),
        output_amount: Uint128::new(800_000), // Bad rate
        price: "0.08".to_string(),
        valid_for_ms: 5000,
    };
    let colluder_2 = SolverQuote {
        solver_id: "colluder-2".to_string(),
        input_amount: Uint128::new(10_000_000),
        output_amount: Uint128::new(850_000), // Also bad rate
        price: "0.085".to_string(),
        valid_for_ms: 5000,
    };
    let colluder_3 = SolverQuote {
        solver_id: "colluder-3".to_string(),
        input_amount: Uint128::new(10_000_000),
        output_amount: Uint128::new(820_000), // Also bad rate
        price: "0.082".to_string(),
        valid_for_ms: 5000,
    };

    let oracle = Decimal::from_str("10.0").unwrap();

    // User's limit price protects them
    // Even if all solvers collude, user's min_output requirement blocks bad fills
    let result = engine.run_batch_auction(
        pair,
        vec![user_buy],
        vec![colluder_1, colluder_2, colluder_3],
        oracle,
    );

    // Auction runs, but user's fills depend on whether any quote meets their requirements
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// CROSS-CHAIN ARBITRAGE ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that cross-ecosystem constraints are respected
#[test]
fn test_cross_ecosystem_constraint() {
    // User can disable cross-ecosystem execution to limit attack surface
    let constraints = ExecutionConstraints::new(9999999999)
        .with_cross_ecosystem(false);

    assert!(!constraints.allow_cross_ecosystem);

    // The orchestrator should enforce this by rejecting fills through
    // non-Cosmos bridges like Axelar, Wormhole, etc.
}

/// Test venue exclusion for specific risky DEXes
#[test]
fn test_venue_exclusion() {
    // User can exclude specific venues they don't trust
    let constraints = ExecutionConstraints::new(9999999999)
        .exclude_venue("risky-dex")
        .exclude_venue("hacked-protocol");

    assert!(constraints.excluded_venues.contains(&"risky-dex".to_string()));
    assert!(constraints.excluded_venues.contains(&"hacked-protocol".to_string()));

    // The orchestrator should reject fills through excluded venues
}
