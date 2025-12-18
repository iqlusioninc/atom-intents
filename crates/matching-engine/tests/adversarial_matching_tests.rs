/// Adversarial tests for the matching engine
///
/// These tests simulate attacks where things could go horribly wrong:
/// - Price manipulation attacks
/// - Limit price bypass attempts
/// - Order flooding attacks
/// - Self-trading/wash trading
/// - Division by zero / overflow attacks
/// - Oracle manipulation scenarios

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
// PRICE MANIPULATION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that oracle price outside user's limit is rejected
#[test]
fn test_oracle_manipulation_attack_buy_side_rejected() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Buyer willing to pay max 10 USDC/ATOM (limit 0.1 ATOM/USDC)
    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );

    // Seller wants at least 10 USDC/ATOM
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    // ATTACK: Oracle reports manipulated price of 15 USDC/ATOM
    let manipulated_oracle = Decimal::from_str("15.0").unwrap();

    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], manipulated_oracle);

    // With midpoint pricing: limits are compatible (10 >= 10), midpoint = 10
    // Oracle deviation = |10 - 15| / 15 = 33% > 10% threshold
    // Result: auction succeeds but no fills due to sanity check
    assert!(result.is_ok());
    assert!(result.unwrap().internal_fills.is_empty(), "High oracle deviation prevents match");
}

/// Test that oracle price outside user's limit is rejected (sell side)
#[test]
fn test_oracle_manipulation_attack_sell_side_rejected() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Buyer willing to pay up to 12 USDC/ATOM
    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 12_000_000, "uatom", 1_000_000, "0.083",
    );

    // Seller wants at least 11 USDC/ATOM - sets limit_price to 11.0
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 11_000_000, "11.0",
    );

    // ATTACK: Oracle reports manipulated price of 10 USDC/ATOM
    let manipulated_oracle = Decimal::from_str("10.0").unwrap();

    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], manipulated_oracle);

    // With midpoint pricing: buyer max = 12.05, seller min = 11
    // Limits cross, midpoint = 11.525 USDC/ATOM
    // Oracle deviation = |11.525 - 10| / 10 = 15.25% > 10% threshold
    // Result: auction succeeds but no fills due to sanity check
    assert!(result.is_ok());
    assert!(result.unwrap().internal_fills.is_empty(), "High oracle deviation prevents match");
}

/// Test that zero oracle price is handled safely
#[test]
fn test_zero_oracle_price_handled() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    // ATTACK: Zero oracle price
    let zero_oracle = Decimal::ZERO;

    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], zero_oracle);

    // With midpoint pricing: zero oracle skips the sanity check
    // Limits are compatible (10 >= 10), so they match at midpoint = 10
    // Result: auction succeeds with fills (oracle check bypassed when oracle is 0)
    assert!(result.is_ok());
    assert!(!result.unwrap().internal_fills.is_empty(), "Zero oracle skips sanity check, limits match");
}

/// Test that negative-like extreme oracle price is handled
#[test]
fn test_extreme_oracle_price_handled() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    // ATTACK: Extremely large oracle price
    let extreme_oracle = Decimal::from_str("999999999999.0").unwrap();

    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], extreme_oracle);

    // With midpoint pricing: limits are compatible, midpoint = 10
    // Oracle deviation = |10 - 999999999999| / 999999999999 ≈ 100% > 10%
    // Result: auction succeeds but no fills due to extreme sanity check failure
    assert!(result.is_ok());
    assert!(result.unwrap().internal_fills.is_empty(), "Extreme oracle deviation prevents match");
}

// ═══════════════════════════════════════════════════════════════════════════
// LIMIT PRICE BYPASS ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test zero limit price is rejected
#[test]
fn test_zero_limit_price_buy_rejected() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // ATTACK: Zero limit price (would mean infinite price tolerance)
    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.0",
    );
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], oracle);

    // With midpoint pricing: zero limit causes division by zero (1/0)
    // Code handles this by skipping the buy intent (buy_limit.is_zero() check)
    // Result: auction succeeds but no fills (buy skipped, no match)
    assert!(result.is_ok());
    assert!(result.unwrap().internal_fills.is_empty(), "Zero limit buy skipped, no match");
}

/// Test that limit price string parsing doesn't allow injection
#[test]
fn test_malformed_limit_price_rejected() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // ATTACK: Malformed limit price
    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "not_a_number",
    );
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], oracle);

    // MUST reject malformed price
    assert!(result.is_err());
}

/// Test empty limit price is rejected
#[test]
fn test_empty_limit_price_rejected() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "",
    );
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], oracle);

    // MUST reject empty price
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// OVERFLOW / UNDERFLOW ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test large (but reasonable) amounts don't cause overflow
/// Note: The rust_decimal library has limits around 28 significant digits,
/// so values near u128::MAX will cause overflow. This is a known limitation.
/// Typical token amounts in crypto (even large ones) are well within bounds.
#[test]
fn test_large_amounts_no_overflow() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Large but reasonable amount (1 trillion with 18 decimals = 10^30)
    // This represents realistic "whale" transactions
    let large_amount: u128 = 1_000_000_000_000_000_000_000_000; // 10^24 (1M with 18 decimals)

    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", large_amount, "uatom", large_amount / 10, "0.1",
    );
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", large_amount / 10, "uusdc", large_amount, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();

    // Should not panic or overflow for reasonable amounts
    let _result = engine.run_batch_auction(pair, vec![buy, sell], vec![], oracle);
}

/// Test zero amounts are handled
#[test]
fn test_zero_amounts_handled() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Zero input amount
    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 0, "uatom", 0, "0.1",
    );
    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 0, "uusdc", 0, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], oracle);

    // Should handle gracefully (no division by zero)
    assert!(result.is_ok());
    let auction = result.unwrap();
    // No fills expected for zero amounts
    assert!(auction.internal_fills.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// WASH TRADING / SELF-TRADING ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that self-trading is detected (same user on both sides)
#[test]
fn test_self_trading_same_user() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // ATTACK: Same user placing both buy and sell
    let buy = make_test_intent(
        "buy-1", "cosmos1sameuser", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );
    let sell = make_test_intent(
        "sell-1", "cosmos1sameuser", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![buy, sell], vec![], oracle);

    // Currently the engine doesn't block self-trading at the matching level
    // This is a potential vulnerability - application layer must check
    // For now, verify no crash
    assert!(result.is_ok());

    // TODO: Consider adding self-trade prevention
}

// ═══════════════════════════════════════════════════════════════════════════
// SOLVER MANIPULATION ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test malicious solver with zero price quote
#[test]
fn test_solver_zero_price_quote() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Only a buy (needs solver)
    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );

    // ATTACK: Solver offers zero price
    let malicious_quote = SolverQuote {
        solver_id: "malicious-solver".to_string(),
        input_amount: Uint128::new(10_000_000),
        output_amount: Uint128::zero(),
        price: "0.0".to_string(),
        valid_for_ms: 5000,
    };

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![buy], vec![malicious_quote], oracle);

    // Should handle gracefully
    assert!(result.is_ok());
}

/// Test solver with extremely high price (price gouging)
#[test]
fn test_solver_extreme_price_gouging() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    let sell = make_test_intent(
        "sell-1", "seller", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    // ATTACK: Solver offers extreme price
    let gouging_quote = SolverQuote {
        solver_id: "gouging-solver".to_string(),
        input_amount: Uint128::new(1_000_000),
        output_amount: Uint128::new(1), // Terrible exchange rate
        price: "0.000001".to_string(),
        valid_for_ms: 5000,
    };

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![sell], vec![gouging_quote], oracle);

    // Should process but user gets bad rate (within their limit though)
    assert!(result.is_ok());
}

/// Test solver with malformed price string
#[test]
fn test_solver_malformed_price() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );

    // ATTACK: Malformed price in quote
    let malformed_quote = SolverQuote {
        solver_id: "malformed-solver".to_string(),
        input_amount: Uint128::new(10_000_000),
        output_amount: Uint128::new(100_000_000),
        price: "not-a-number".to_string(),
        valid_for_ms: 5000,
    };

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![buy], vec![malformed_quote], oracle);

    // Should handle gracefully (malformed prices go to MAX/ZERO during sort)
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// ORDER BOOK STATE ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test processing same intent twice doesn't corrupt state
#[test]
fn test_duplicate_intent_processing() {
    let mut engine = MatchingEngine::new();

    let intent = make_test_intent(
        "dup-1", "user", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0",
    );

    // Process same intent twice
    let result1 = engine.process_intent(&intent, 0);
    let result2 = engine.process_intent(&intent, 1);

    // Both should succeed (book tracks by order, not dedup)
    assert!(result1.is_ok());
    assert!(result2.is_ok());

    // But this is potentially problematic - application layer must prevent replay
}

/// Test empty batch auction doesn't crash
#[test]
fn test_empty_batch_auction() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");
    let oracle = Decimal::from_str("10.0").unwrap();

    let result = engine.run_batch_auction(pair, vec![], vec![], oracle);

    assert!(result.is_ok());
    let auction = result.unwrap();
    assert!(auction.internal_fills.is_empty());
    assert!(auction.solver_fills.is_empty());
}

/// Test many orders in single auction (DOS attempt)
#[test]
fn test_many_orders_dos_attempt() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Create many orders
    let mut intents = Vec::new();
    for i in 0..100 {
        if i % 2 == 0 {
            intents.push(make_test_intent(
                &format!("buy-{}", i),
                &format!("buyer-{}", i),
                "uusdc",
                10_000_000,
                "uatom",
                1_000_000,
                "0.1",
            ));
        } else {
            intents.push(make_test_intent(
                &format!("sell-{}", i),
                &format!("seller-{}", i),
                "uatom",
                1_000_000,
                "uusdc",
                10_000_000,
                "10.0",
            ));
        }
    }

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, intents, vec![], oracle);

    // Should complete without timeout/crash
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// CLEARING PRICE MANIPULATION TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that clearing price calculation handles edge cases
#[test]
fn test_clearing_price_edge_cases() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");

    // Only buys, no sells
    let buy = make_test_intent(
        "buy-1", "buyer", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1",
    );

    let oracle = Decimal::from_str("10.0").unwrap();
    let result = engine.run_batch_auction(pair, vec![buy], vec![], oracle);

    assert!(result.is_ok());
    let auction = result.unwrap();

    // With no matches, clearing price should be oracle price
    assert_eq!(auction.clearing_price, "10.0");
}

/// Test epoch ID increments correctly even with failures
#[test]
fn test_epoch_increments_on_success() {
    let mut engine = MatchingEngine::new();
    let pair = TradingPair::new("uatom", "uusdc");
    let oracle = Decimal::from_str("10.0").unwrap();

    // First auction
    let result1 = engine.run_batch_auction(pair.clone(), vec![], vec![], oracle);
    assert_eq!(result1.unwrap().epoch_id, 1);

    // Second auction
    let result2 = engine.run_batch_auction(pair.clone(), vec![], vec![], oracle);
    assert_eq!(result2.unwrap().epoch_id, 2);

    // Third auction
    let result3 = engine.run_batch_auction(pair, vec![], vec![], oracle);
    assert_eq!(result3.unwrap().epoch_id, 3);
}
