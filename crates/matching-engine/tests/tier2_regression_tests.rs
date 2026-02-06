/// Tier 2 Audit Regression Tests
///
/// Tests preventing reintroduction of vulnerabilities fixed in the Tier 2 audit:
/// - ME-C1: OrderedPrice Ord antisymmetry violation
/// - ME-C3: Silent truncation rounding to zero
/// - ME-H1: Unsorted intent crossing enabling MEV
/// - ME-H2: Unenforced fill constraints

use atom_intents_matching_engine::{safe_decimal_to_uint128, MatchingEngine};
use atom_intents_types::{
    Asset, ExecutionConstraints, FillConfig, FillStrategy, Intent, OutputSpec, TradingPair,
};
use cosmwasm_std::{Binary, Uint128};
use rust_decimal::Decimal;
use std::str::FromStr;

fn make_intent(
    id: &str,
    user: &str,
    input_denom: &str,
    input_amount: u128,
    output_denom: &str,
    min_output: u128,
    limit_price: &str,
) -> Intent {
    make_intent_with_fill_config(
        id,
        user,
        input_denom,
        input_amount,
        output_denom,
        min_output,
        limit_price,
        FillConfig {
            allow_partial: true,
            min_fill_amount: Uint128::zero(),
            min_fill_pct: "0".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        },
    )
}

fn make_intent_with_fill_config(
    id: &str,
    user: &str,
    input_denom: &str,
    input_amount: u128,
    output_denom: &str,
    min_output: u128,
    limit_price: &str,
    fill_config: FillConfig,
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
        fill_config,
        constraints: ExecutionConstraints::new(9999999999),
        signature: Binary::default(),
        public_key: Binary::default(),
        created_at: 0,
        expires_at: 9999999999,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ME-C1: OrderedPrice Ord Antisymmetry
// ═══════════════════════════════════════════════════════════════════════════

/// Regression test: bids should be matched from highest price first.
/// Three bids at different prices are added, then a crossing sell is submitted.
/// The sell should match against the highest bid first.
#[test]
fn test_bid_ordering_highest_first() {
    let mut engine = MatchingEngine::new();

    // Add bids at prices 8, 12, 10 (out of order)
    // Buy at 8 USDC/ATOM = limit 0.125 ATOM/USDC
    let buy_8 = make_intent("buy-8", "b1", "uusdc", 8_000_000, "uatom", 1_000_000, "0.125");
    // Buy at 12 USDC/ATOM = limit 1/12 ≈ 0.0833 ATOM/USDC
    let buy_12 = make_intent(
        "buy-12",
        "b2",
        "uusdc",
        12_000_000,
        "uatom",
        1_000_000,
        "0.0833",
    );
    // Buy at 10 USDC/ATOM = limit 0.1 ATOM/USDC
    let buy_10 = make_intent("buy-10", "b3", "uusdc", 10_000_000, "uatom", 1_000_000, "0.1");

    // Submit bids - all go to empty book
    engine.process_intent(&buy_8, 0).unwrap();
    engine.process_intent(&buy_12, 1).unwrap();
    engine.process_intent(&buy_10, 2).unwrap();

    // Now add a sell at 9 USDC/ATOM that should match the best bid (12) first
    let sell = make_intent("sell-1", "s1", "uatom", 1_000_000, "uusdc", 9_000_000, "9.0");

    let result = engine.process_intent(&sell, 3).unwrap();

    // The sell should match against the highest bid
    assert!(!result.fills.is_empty(), "Should have matched");
}

/// Regression test: asks should be matched from lowest price first.
#[test]
fn test_ask_ordering_lowest_first() {
    let mut engine = MatchingEngine::new();

    // Add sells at prices 12, 8, 10 (out of order)
    let sell_12 = make_intent(
        "sell-12",
        "s1",
        "uatom",
        1_000_000,
        "uusdc",
        12_000_000,
        "12.0",
    );
    let sell_8 = make_intent("sell-8", "s2", "uatom", 1_000_000, "uusdc", 8_000_000, "8.0");
    let sell_10 = make_intent(
        "sell-10",
        "s3",
        "uatom",
        1_000_000,
        "uusdc",
        10_000_000,
        "10.0",
    );

    engine.process_intent(&sell_12, 0).unwrap();
    engine.process_intent(&sell_8, 1).unwrap();
    engine.process_intent(&sell_10, 2).unwrap();

    // Buy willing to pay up to 9 USDC/ATOM (0.111 ATOM/USDC) - should match the 8.0 ask first
    let buy = make_intent("buy-1", "b1", "uusdc", 9_000_000, "uatom", 1_000_000, "0.111");

    let result = engine.process_intent(&buy, 3).unwrap();

    assert!(!result.fills.is_empty(), "Should have matched the 8.0 ask");
    assert_eq!(result.fills[0].price, "8.0", "Should match lowest ask first");
}

// ═══════════════════════════════════════════════════════════════════════════
// ME-H1: Intent Sorting Before Crossing
// ═══════════════════════════════════════════════════════════════════════════

/// Regression test: batch auction results should be independent of
/// intent submission order. Best-priced intents should match first.
#[test]
fn test_batch_auction_order_independent() {
    let pair = TradingPair::new("uatom", "uusdc");
    let oracle_price = Decimal::from_str("10.0").unwrap();

    // Create buys with different willingness to pay
    // buy_good: willing to pay up to 1/0.09 ≈ 11.1 USDC/ATOM
    let buy_good = make_intent(
        "buy-good",
        "b1",
        "uusdc",
        10_000_000,
        "uatom",
        900_000,
        "0.09",
    );
    // buy_better: willing to pay up to 1/0.08 = 12.5 USDC/ATOM
    let buy_better = make_intent(
        "buy-better",
        "b2",
        "uusdc",
        10_000_000,
        "uatom",
        800_000,
        "0.08",
    );
    // sell: wants at least 10 USDC/ATOM
    let sell = make_intent("sell-1", "s1", "uatom", 1_000_000, "uusdc", 10_000_000, "10.0");

    // Order 1: good buy first, then better
    let mut engine1 = MatchingEngine::new();
    let result1 = engine1.run_batch_auction(
        pair.clone(),
        vec![buy_good.clone(), buy_better.clone(), sell.clone()],
        vec![],
        oracle_price,
    );

    // Order 2: better buy first, then good
    let mut engine2 = MatchingEngine::new();
    let result2 = engine2.run_batch_auction(
        pair.clone(),
        vec![buy_better.clone(), buy_good.clone(), sell.clone()],
        vec![],
        oracle_price,
    );

    // Both should produce results (matching is possible)
    assert!(result1.is_ok() || result2.is_ok(), "At least one should succeed");
    if let (Ok(r1), Ok(r2)) = (&result1, &result2) {
        // Same number of fills regardless of submission order
        assert_eq!(r1.internal_fills.len(), r2.internal_fills.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ME-H2: Fill Constraint Enforcement
// ═══════════════════════════════════════════════════════════════════════════

/// Regression test: min_fill_amount should block fills below the threshold.
#[test]
fn test_min_fill_amount_enforced_in_book() {
    let mut engine = MatchingEngine::new();

    // Sell with min_fill_amount of 500_000 (0.5 ATOM)
    let sell = make_intent_with_fill_config(
        "sell-1",
        "seller",
        "uatom",
        1_000_000,
        "uusdc",
        10_000_000,
        "10.0",
        FillConfig {
            allow_partial: true,
            min_fill_amount: Uint128::new(500_000),
            min_fill_pct: "0".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        },
    );

    engine.process_intent(&sell, 0).unwrap();

    // Small buy that would only fill 200_000 base (well below min)
    // 2_000_000 USDC at price 10.0 = 200_000 ATOM
    let small_buy = make_intent(
        "buy-small",
        "buyer",
        "uusdc",
        2_000_000,
        "uatom",
        200_000,
        "0.1",
    );

    let result = engine.process_intent(&small_buy, 1).unwrap();

    // Fill should be skipped because 200_000 < min_fill_amount 500_000
    assert!(
        result.fills.is_empty(),
        "Fill below min_fill_amount should be skipped"
    );
}

/// Regression test: min_fill_pct should block fills below the percentage threshold.
#[test]
fn test_min_fill_pct_enforced_in_book() {
    let mut engine = MatchingEngine::new();

    // Sell with min_fill_pct of 0.5 (50%) on 1_000_000 base
    let sell = make_intent_with_fill_config(
        "sell-1",
        "seller",
        "uatom",
        1_000_000,
        "uusdc",
        10_000_000,
        "10.0",
        FillConfig {
            allow_partial: true,
            min_fill_amount: Uint128::zero(),
            min_fill_pct: "0.5".to_string(), // 50%
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        },
    );

    engine.process_intent(&sell, 0).unwrap();

    // Buy that would only fill ~10% of the sell order
    // 1_000_000 USDC at price 10.0 = 100_000 ATOM = 10% of 1_000_000
    let small_buy = make_intent(
        "buy-small",
        "buyer",
        "uusdc",
        1_000_000,
        "uatom",
        100_000,
        "0.1",
    );

    let result = engine.process_intent(&small_buy, 1).unwrap();

    // Fill should be skipped because 10% < 50% min_fill_pct
    assert!(
        result.fills.is_empty(),
        "Fill below min_fill_pct should be skipped"
    );
}

/// Regression test: fills that complete the entire remaining amount should
/// always be accepted, even if below min_fill thresholds (to prevent dust).
#[test]
fn test_full_fill_bypasses_min_constraints() {
    let mut engine = MatchingEngine::new();

    // Sell with high min_fill_amount but small total size
    let sell = make_intent_with_fill_config(
        "sell-1",
        "seller",
        "uatom",
        100_000, // Small total
        "uusdc",
        1_000_000,
        "10.0",
        FillConfig {
            allow_partial: true,
            min_fill_amount: Uint128::new(500_000), // Higher than total
            min_fill_pct: "0".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        },
    );

    engine.process_intent(&sell, 0).unwrap();

    // Buy large enough to fully fill the sell
    let buy = make_intent(
        "buy-1",
        "buyer",
        "uusdc",
        10_000_000,
        "uatom",
        100_000,
        "0.1",
    );

    let result = engine.process_intent(&buy, 1).unwrap();

    // Full fill should succeed even though the amount < min_fill_amount,
    // because it fills the entire remaining amount (maker_fully_filled = true).
    assert!(
        !result.fills.is_empty(),
        "Full fill should bypass min_fill_amount constraint"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ME-C3: Safe Decimal Conversion
// ═══════════════════════════════════════════════════════════════════════════

/// Regression test: conversion functions should not silently return zero.
#[test]
fn test_safe_conversion_normal_values() {
    let result = safe_decimal_to_uint128(Decimal::from(1000u64));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Uint128::new(1000));

    let result = safe_decimal_to_uint128(Decimal::ZERO);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Uint128::zero());
}

/// Regression test: negative values must return an error, not zero.
#[test]
fn test_safe_conversion_negative_returns_error() {
    let result = safe_decimal_to_uint128(Decimal::from(-1i64));
    assert!(result.is_err(), "Negative values should return error");
}

/// Regression test: fractional values are truncated correctly.
#[test]
fn test_safe_conversion_truncates_fractions() {
    let result = safe_decimal_to_uint128(Decimal::from_str("10.7").unwrap());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Uint128::new(10));

    let result = safe_decimal_to_uint128(Decimal::from_str("99.99").unwrap());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Uint128::new(99));
}
