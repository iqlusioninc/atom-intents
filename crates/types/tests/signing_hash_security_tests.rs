/// Security tests for signing hash
///
/// These tests verify that ALL security-critical fields are included in the signing hash,
/// preventing signature bypass attacks where an attacker could modify execution parameters
/// after the user has signed the intent.
///
/// CRITICAL: Each test verifies that changing a specific field CHANGES the signing hash.
/// If any of these tests fail, it means that field is NOT protected by the signature,
/// which is a critical security vulnerability.

use atom_intents_types::{
    Asset, ExecutionConstraints, FillConfig, FillStrategy, Intent, OutputSpec,
};
use cosmwasm_std::Uint128;

/// Helper to create a baseline intent for testing
fn create_baseline_unsigned() -> atom_intents_types::UnsignedIntent {
    Intent::builder()
        .user("cosmos1user123")
        .input(Asset {
            chain_id: "cosmoshub-4".to_string(),
            denom: "uatom".to_string(),
            amount: Uint128::new(1000000),
        })
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5000000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig {
            allow_partial: false,
            min_fill_amount: Uint128::new(100000),
            min_fill_pct: "0.5".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        })
        .constraints(
            ExecutionConstraints::new(1000000)
                .with_max_hops(3)
                .with_cross_ecosystem(false)
                .exclude_venue("malicious-dex"),
        )
        .nonce(42)
        .build(100, 2000)
        .unwrap()
}

/// Helper to sign an unsigned intent with a test key
fn sign_intent(unsigned: atom_intents_types::UnsignedIntent) -> Intent {
    let private_key = [0x42; 32];
    unsigned.sign_with_key(&private_key).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════
// FILL CONFIG SECURITY TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_changing_allow_partial_changes_signature() {
    // Create two intents that differ only in allow_partial
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.fill_config.allow_partial = false;

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.fill_config.allow_partial = true;

    // Sign both
    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    // Signatures MUST be different
    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: allow_partial not included in signing hash!"
    );
    assert_ne!(
        signed1.signature,
        signed2.signature,
        "Signatures should differ when allow_partial changes"
    );

    // Both should verify independently
    assert!(signed1.verify().unwrap());
    assert!(signed2.verify().unwrap());

    // Cross-verification should fail
    let mut tampered = signed1.clone();
    tampered.fill_config.allow_partial = true;
    assert!(
        tampered.verify().is_err(),
        "SECURITY FAILURE: Signature should be invalid after changing allow_partial!"
    );
}

#[test]
fn test_changing_min_fill_amount_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.fill_config.min_fill_amount = Uint128::new(100000);

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.fill_config.min_fill_amount = Uint128::new(200000);

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: min_fill_amount not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_changing_min_fill_pct_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.fill_config.min_fill_pct = "0.5".to_string();

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.fill_config.min_fill_pct = "0.9".to_string();

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: min_fill_pct not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_changing_aggregation_window_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.fill_config.aggregation_window_ms = 5000;

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.fill_config.aggregation_window_ms = 10000;

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: aggregation_window_ms not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_changing_fill_strategy_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.fill_config.strategy = FillStrategy::Eager;

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.fill_config.strategy = FillStrategy::AllOrNothing;

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: fill_strategy not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_changing_strategy_variant_params_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.fill_config.strategy = FillStrategy::MinimumThenEager {
        min_pct: "0.5".to_string(),
    };

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.fill_config.strategy = FillStrategy::MinimumThenEager {
        min_pct: "0.9".to_string(),
    };

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: fill_strategy variant parameters not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

// ═══════════════════════════════════════════════════════════════════════════
// EXECUTION CONSTRAINTS SECURITY TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_changing_max_hops_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.constraints.max_hops = Some(3);

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.max_hops = Some(10);

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: max_hops not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);

    // Verify tampering detection
    let mut tampered = signed1.clone();
    tampered.constraints.max_hops = Some(999);
    assert!(
        tampered.verify().is_err(),
        "SECURITY FAILURE: Signature should be invalid after changing max_hops!"
    );
}

#[test]
fn test_changing_max_hops_from_some_to_none_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.constraints.max_hops = Some(3);

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.max_hops = None;

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: max_hops Some/None distinction not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_changing_excluded_venues_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.constraints.excluded_venues = vec!["malicious-dex".to_string()];

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.excluded_venues = vec!["different-dex".to_string()];

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: excluded_venues not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);

    // Verify tampering detection
    let mut tampered = signed1.clone();
    tampered.constraints.excluded_venues = vec![];
    assert!(
        tampered.verify().is_err(),
        "SECURITY FAILURE: Signature should be invalid after changing excluded_venues!"
    );
}

#[test]
fn test_adding_excluded_venue_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.constraints.excluded_venues = vec!["dex-a".to_string()];

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.excluded_venues = vec!["dex-a".to_string(), "dex-b".to_string()];

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: Adding to excluded_venues not detected in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_excluded_venues_order_is_normalized() {
    // Test that venue order doesn't matter (sorted for determinism)
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.constraints.excluded_venues = vec!["dex-a".to_string(), "dex-b".to_string()];

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.excluded_venues = vec!["dex-b".to_string(), "dex-a".to_string()];

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    // Hashes should be THE SAME (sorted before hashing)
    assert_eq!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "Excluded venues should be sorted for deterministic hashing"
    );
    assert_eq!(
        signed1.signature,
        signed2.signature,
        "Same excluded venues in different order should produce same signature"
    );
}

#[test]
fn test_changing_allow_cross_ecosystem_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.constraints.allow_cross_ecosystem = false;

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.allow_cross_ecosystem = true;

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: allow_cross_ecosystem not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);

    // This is a critical security check: attacker shouldn't be able to
    // enable cross-ecosystem execution on a Cosmos-only intent
    let mut tampered = signed1.clone();
    tampered.constraints.allow_cross_ecosystem = true;
    assert!(
        tampered.verify().is_err(),
        "CRITICAL SECURITY FAILURE: Signature should be invalid after enabling cross-ecosystem!"
    );
}

#[test]
fn test_changing_max_solver_fee_bps_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.constraints.max_solver_fee_bps = Some(50);

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.max_solver_fee_bps = Some(100);

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: max_solver_fee_bps not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_changing_max_bridge_time_secs_changes_signature() {
    let mut unsigned1 = create_baseline_unsigned();
    unsigned1.constraints.max_bridge_time_secs = Some(300);

    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.max_bridge_time_secs = Some(600);

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "SECURITY FAILURE: max_bridge_time_secs not included in signing hash!"
    );
    assert_ne!(signed1.signature, signed2.signature);
}

// ═══════════════════════════════════════════════════════════════════════════
// COMPREHENSIVE ATTACK SCENARIO TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_attack_scenario_partial_fill_exploitation() {
    // Scenario: User signs intent with allow_partial=false (all-or-nothing)
    // Attacker tries to change to allow_partial=true to enable partial fills
    // and drain the user's funds incrementally

    let mut unsigned = create_baseline_unsigned();
    unsigned.fill_config.allow_partial = false; // User wants all-or-nothing

    let private_key = [0x42; 32];
    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Attacker modifies allow_partial
    signed.fill_config.allow_partial = true;

    // Verification MUST fail
    let result = signed.verify();
    assert!(
        result.is_err(),
        "CRITICAL ATTACK VECTOR: Attacker changed allow_partial from false to true!"
    );
    assert_eq!(
        result.unwrap_err(),
        atom_intents_types::VerificationError::VerificationFailed
    );
}

#[test]
fn test_attack_scenario_remove_max_hops_constraint() {
    // Scenario: User signs intent with max_hops=3 (short path only)
    // Attacker removes constraint to enable longer, more costly routes

    let mut unsigned = create_baseline_unsigned();
    unsigned.constraints.max_hops = Some(3);

    let private_key = [0x42; 32];
    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Attacker removes the constraint
    signed.constraints.max_hops = None;

    // Verification MUST fail
    let result = signed.verify();
    assert!(
        result.is_err(),
        "CRITICAL ATTACK VECTOR: Attacker removed max_hops constraint!"
    );
}

#[test]
fn test_attack_scenario_remove_excluded_venues() {
    // Scenario: User signs intent excluding "malicious-dex"
    // Attacker removes the exclusion to route through malicious venue

    let mut unsigned = create_baseline_unsigned();
    unsigned.constraints.excluded_venues = vec!["malicious-dex".to_string()];

    let private_key = [0x42; 32];
    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Attacker removes excluded venues
    signed.constraints.excluded_venues = vec![];

    // Verification MUST fail
    let result = signed.verify();
    assert!(
        result.is_err(),
        "CRITICAL ATTACK VECTOR: Attacker removed excluded_venues!"
    );
}

#[test]
fn test_attack_scenario_enable_cross_ecosystem() {
    // Scenario: User signs Cosmos-only intent (allow_cross_ecosystem=false)
    // Attacker enables cross-ecosystem to route through less secure bridges

    let mut unsigned = create_baseline_unsigned();
    unsigned.constraints.allow_cross_ecosystem = false;

    let private_key = [0x42; 32];
    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Attacker enables cross-ecosystem
    signed.constraints.allow_cross_ecosystem = true;

    // Verification MUST fail
    let result = signed.verify();
    assert!(
        result.is_err(),
        "CRITICAL ATTACK VECTOR: Attacker enabled cross-ecosystem execution!"
    );
}

#[test]
fn test_attack_scenario_change_fill_strategy() {
    // Scenario: User signs with AllOrNothing strategy
    // Attacker changes to Eager to enable unfavorable partial fills

    let mut unsigned = create_baseline_unsigned();
    unsigned.fill_config.strategy = FillStrategy::AllOrNothing;

    let private_key = [0x42; 32];
    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Attacker changes strategy
    signed.fill_config.strategy = FillStrategy::Eager;

    // Verification MUST fail
    let result = signed.verify();
    assert!(
        result.is_err(),
        "CRITICAL ATTACK VECTOR: Attacker changed fill strategy!"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// REGRESSION TESTS - Ensure original fields still protected
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_original_fields_still_protected_deadline() {
    let unsigned1 = create_baseline_unsigned();
    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.constraints.deadline = 999999;

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "Regression: deadline should still be in signing hash"
    );
}

#[test]
fn test_original_fields_still_protected_input_amount() {
    let unsigned1 = create_baseline_unsigned();
    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.input.amount = Uint128::new(9999999);

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "Regression: input.amount should still be in signing hash"
    );
}

#[test]
fn test_original_fields_still_protected_output_recipient() {
    let unsigned1 = create_baseline_unsigned();
    let mut unsigned2 = create_baseline_unsigned();
    unsigned2.output.recipient = "osmo1attacker".to_string();

    let signed1 = sign_intent(unsigned1);
    let signed2 = sign_intent(unsigned2);

    assert_ne!(
        signed1.signing_hash(),
        signed2.signing_hash(),
        "Regression: output.recipient should still be in signing hash"
    );
}
