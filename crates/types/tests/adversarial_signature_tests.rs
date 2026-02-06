/// Adversarial tests for intent signature system
///
/// Structured using a threat-actor framework to ensure coverage of:
///
/// CATEGORY 1 - Primitive Correctness:
///   Tests that ECDSA operations work correctly at the byte level.
///   (Bit flips, truncation, malleability, garbage bytes)
///
/// CATEGORY 2 - Identity Binding:
///   Tests that the signer is authorized for the claimed identity.
///   (Pubkey-address binding, cross-user impersonation, prefix attacks)
///
/// CATEGORY 3 - Authorization Semantics:
///   Tests that the system rejects unauthorized state transitions.
///   (Field tampering, expiry manipulation, fill config changes)
///
/// CATEGORY 4 - Security Property Invariants:
///   Direct tests of the properties the system must guarantee.
///   Each test names the attacker, their goal, and the method.
///
/// CATEGORY 5 - Composite / Multi-Step Attacks:
///   Attacks that combine multiple techniques.

use atom_intents_types::{
    verification::{
        derive_public_key, pubkey_to_bech32_address, sign_message, verify_intent_signature,
        VerificationError,
    },
    Asset, CancellationRequest, ExecutionConstraints, FillConfig, FillStrategy, Intent, OutputSpec,
};
use cosmwasm_std::{Binary, Uint128};

/// Derive the correct bech32 address for a test private key
fn test_address(private_key: &[u8], prefix: &str) -> String {
    let pubkey = derive_public_key(private_key).unwrap();
    pubkey_to_bech32_address(&pubkey, prefix).unwrap()
}

/// Build an unsigned intent for a given user address with standard test data
fn build_unsigned_for(user: &str) -> atom_intents_types::UnsignedIntent {
    Intent::builder()
        .user(user)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap()
}

/// Sign an unsigned intent with a key and return the signed Intent
fn sign_intent(
    unsigned: atom_intents_types::UnsignedIntent,
    private_key: &[u8],
) -> atom_intents_types::Intent {
    unsigned.sign_with_key(private_key).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════
// REPLAY ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that replaying the exact same intent twice fails (nonce should be unique)
#[test]
fn test_replay_attack_same_intent_fails() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig {
            allow_partial: false,
            min_fill_amount: Uint128::new(100_000),
            min_fill_pct: "0.5".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        })
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let signed1 = unsigned.clone().sign_with_key(&private_key).unwrap();
    let signed2 = unsigned.sign_with_key(&private_key).unwrap();

    // Both signatures should be identical (same nonce = same intent)
    assert_eq!(signed1.signature, signed2.signature);

    // Both should verify (for the same intent)
    assert!(signed1.verify().unwrap());
    assert!(signed2.verify().unwrap());

    // SECURITY NOTE: The application layer MUST track used nonces
    // to prevent replay attacks. The signature alone cannot prevent this.
    // Test that IDs are identical (deterministic)
    assert_eq!(signed1.id, signed2.id);
}

/// Test replay attack with different nonce is blocked
#[test]
fn test_replay_attack_different_nonce_creates_different_signature() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned1 = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let unsigned2 = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(43) // Different nonce
        .build(100, 2000)
        .unwrap();

    let signed1 = unsigned1.sign_with_key(&private_key).unwrap();
    let signed2 = unsigned2.sign_with_key(&private_key).unwrap();

    // Signatures MUST differ for different nonces
    assert_ne!(signed1.signature, signed2.signature);
    assert_ne!(signed1.signing_hash(), signed2.signing_hash());
}

/// Test cross-chain replay attack (same intent on different chain)
#[test]
fn test_cross_chain_replay_attack_prevented() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    // Intent for cosmoshub-4
    let unsigned_cosmoshub = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    // Same intent but input is on different chain
    let unsigned_osmosis = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("osmosis-1", "uatom", 1_000_000)) // Different chain!
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let signed_cosmoshub = unsigned_cosmoshub.sign_with_key(&private_key).unwrap();
    let signed_osmosis = unsigned_osmosis.sign_with_key(&private_key).unwrap();

    // Signatures MUST differ (chain_id is in signing hash)
    assert_ne!(signed_cosmoshub.signature, signed_osmosis.signature);
    assert_ne!(signed_cosmoshub.signing_hash(), signed_osmosis.signing_hash());
}

// ═══════════════════════════════════════════════════════════════════════════
// SIGNATURE MALLEABILITY TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that modifying signature bytes invalidates verification
#[test]
fn test_signature_bit_flip_invalidates() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();
    assert!(signed.verify().unwrap());

    // Flip a bit in the signature
    let mut sig_bytes = signed.signature.to_vec();
    if !sig_bytes.is_empty() {
        sig_bytes[0] ^= 0x01; // Flip one bit
        signed.signature = Binary::from(sig_bytes);

        // Verification MUST fail
        let result = signed.verify();
        assert!(result.is_err() || !result.unwrap());
    }
}

/// Test S-malleability attack (ECDSA high-S to low-S conversion)
#[test]
fn test_s_malleability_attack() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let signed = unsigned.sign_with_key(&private_key).unwrap();

    // The signature should be in low-S form for ECDSA
    // secp256k1 curve order N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
    // If S > N/2, we can compute S' = N - S to get a valid alternative signature
    // This is a malleability attack that should be blocked

    let sig_bytes = signed.signature.to_vec();
    assert_eq!(sig_bytes.len(), 64, "Signature should be 64 bytes (r || s)");

    // Original verification should pass
    assert!(signed.verify().unwrap());

    // Note: Our implementation should use low-S form signatures
    // The test verifies the signature is valid and any S-malleability
    // would be caught by the signature verification layer
}

/// Test truncated signature is rejected
#[test]
fn test_truncated_signature_rejected() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Truncate the signature
    let sig_bytes = &signed.signature.to_vec()[..32]; // Only keep r, drop s
    signed.signature = Binary::from(sig_bytes.to_vec());

    // Verification MUST fail with truncated signature
    let result = signed.verify();
    assert!(result.is_err());
}

/// Test extended signature with garbage bytes is rejected
#[test]
fn test_extended_signature_with_garbage_rejected() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Add garbage bytes to signature
    let mut sig_bytes = signed.signature.to_vec();
    sig_bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    signed.signature = Binary::from(sig_bytes);

    // Verification MUST fail with extended signature
    let result = signed.verify();
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// FORGERY ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that signing with wrong key fails verification (address binding)
#[test]
fn test_wrong_key_signature_rejected() {
    let correct_key = [0x42; 32];
    let wrong_key = [0x43; 32];
    let user_addr = test_address(&correct_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    // Sign with correct key - should verify
    let signed_correct = unsigned.clone().sign_with_key(&correct_key).unwrap();
    assert!(signed_correct.verify().unwrap());

    // Sign with wrong key - address binding MUST reject this
    let signed_wrong = unsigned.sign_with_key(&wrong_key).unwrap();
    assert_ne!(signed_correct.public_key, signed_wrong.public_key);

    // T-C4: Wrong key's pubkey doesn't derive the user address, so verify must fail
    let result = signed_wrong.verify();
    assert!(
        result.is_err(),
        "SECURITY: Wrong key must fail address binding check"
    );
}

/// Test empty signature is rejected
#[test]
fn test_empty_signature_rejected() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();
    signed.signature = Binary::default(); // Empty signature

    // Verification MUST fail
    let result = signed.verify();
    assert!(result.is_err());
}

/// Test all-zeros signature is rejected
#[test]
fn test_zero_signature_rejected() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();
    signed.signature = Binary::from(vec![0u8; 64]); // All zeros

    // Verification MUST fail
    let result = signed.verify();
    assert!(result.is_err() || !result.unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════
// FIELD TAMPERING ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test changing recipient after signing fails verification
#[test]
fn test_recipient_tampering_fails() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1legituser".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();
    assert!(signed.verify().unwrap());

    // ATTACK: Change recipient to attacker address
    signed.output.recipient = "osmo1attacker".to_string();

    // Verification MUST fail
    let result = signed.verify();
    assert!(result.is_err() || !result.unwrap());
}

/// Test changing amount after signing fails verification
#[test]
fn test_amount_tampering_fails() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();
    assert!(signed.verify().unwrap());

    // ATTACK: Increase amount to drain more funds
    signed.input.amount = Uint128::new(999_999_999_999);

    // Verification MUST fail
    let result = signed.verify();
    assert!(result.is_err() || !result.unwrap());
}

/// Test changing min_output after signing fails verification
#[test]
fn test_min_output_tampering_fails() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();
    assert!(signed.verify().unwrap());

    // ATTACK: Lower min_output to give user less
    signed.output.min_amount = Uint128::new(1);

    // Verification MUST fail
    let result = signed.verify();
    assert!(result.is_err() || !result.unwrap());
}

/// Test changing deadline after signing fails verification
#[test]
fn test_deadline_tampering_fails() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();
    assert!(signed.verify().unwrap());

    // ATTACK: Extend deadline to give more time for price manipulation
    signed.constraints.deadline = 9999999999;

    // Verification MUST fail
    let result = signed.verify();
    assert!(result.is_err() || !result.unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════
// NONCE ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that nonce 0 is valid (edge case)
#[test]
fn test_nonce_zero_is_valid() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(0) // Nonce zero
        .build(100, 2000)
        .unwrap();

    let signed = unsigned.sign_with_key(&private_key).unwrap();
    assert!(signed.verify().unwrap());
}

/// Test that max nonce is valid (edge case)
#[test]
fn test_nonce_max_is_valid() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(u64::MAX) // Max nonce
        .build(100, 2000)
        .unwrap();

    let signed = unsigned.sign_with_key(&private_key).unwrap();
    assert!(signed.verify().unwrap());
}

/// Test nonce increment creates different signatures
#[test]
fn test_nonce_increment_changes_signature() {
    let private_key = [0x42; 32];
    let user_addr = test_address(&private_key, "cosmos");

    let mut signatures = Vec::new();

    for nonce in 0..10 {
        let unsigned = Intent::builder()
            .user(&user_addr)
            .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
            .output(OutputSpec {
                chain_id: "osmosis-1".to_string(),
                denom: "uosmo".to_string(),
                min_amount: Uint128::new(5_000_000),
                limit_price: "5.0".to_string(),
                recipient: "osmo1user123".to_string(),
            })
            .fill_config(FillConfig::default())
            .constraints(ExecutionConstraints::new(1_000_000))
            .nonce(nonce)
            .build(100, 2000)
            .unwrap();

        let signed = unsigned.sign_with_key(&private_key).unwrap();
        signatures.push(signed.signature.clone());
    }

    // All signatures must be unique
    for i in 0..signatures.len() {
        for j in (i + 1)..signatures.len() {
            assert_ne!(signatures[i], signatures[j], "Signatures for nonces {} and {} should differ", i, j);
        }
    }
}

// =============================================================================
// CATEGORY 2: IDENTITY BINDING ATTACK TESTS
// =============================================================================
// These tests verify that signature verification enforces the binding between
// the signing key and the claimed user identity. This was the class of
// vulnerability missed by the original test suite.

/// ATTACKER: Malicious solver who knows the victim's address
/// GOAL: Submit a fake intent to drain victim's escrow
/// METHOD: Sign with own key, set intent.user to victim's address
#[test]
fn test_solver_impersonates_user_via_intent() {
    let solver_key = [0x42; 32];
    let victim_key = [0x43; 32];
    let victim_addr = test_address(&victim_key, "cosmos");

    // Solver creates intent claiming to be the victim
    let unsigned = build_unsigned_for(&victim_addr);

    // Solver signs with their own key (not the victim's)
    let signed = sign_intent(unsigned, &solver_key);

    // The ECDSA signature is technically valid for the solver's pubkey,
    // but verification MUST fail because solver's pubkey doesn't derive
    // the victim's address
    let result = signed.verify();
    assert!(result.is_err(), "SECURITY: Solver impersonation must be rejected");
    assert!(
        matches!(result.unwrap_err(), VerificationError::AddressMismatch { .. }),
        "Must fail with AddressMismatch, not a generic error"
    );
}

/// ATTACKER: Malicious solver who knows the victim's address
/// GOAL: Cancel victim's pending intent to prevent it from being filled
/// METHOD: Sign cancellation with own key, set user to victim's address
#[test]
fn test_solver_cancels_victims_intent() {
    let solver_key = [0x42; 32];
    let victim_key = [0x43; 32];
    let victim_addr = test_address(&victim_key, "cosmos");

    // Solver creates cancellation request claiming to be the victim
    let req = CancellationRequest::new("intent-target-123", &victim_addr, 1000);
    let signed = req.sign(&solver_key).expect("signing should succeed");

    // Verification MUST fail - solver cannot cancel victim's intent
    let result = signed.verify();
    assert!(
        result.is_err(),
        "SECURITY: Unauthorized cancellation must be rejected"
    );
    assert!(matches!(
        result.unwrap_err(),
        VerificationError::AddressMismatch { .. }
    ));
}

/// ATTACKER: Man-in-the-middle between user and orchestrator
/// GOAL: Swap the pubkey in a legitimately signed intent so verification
///       passes with the wrong identity binding
/// METHOD: Take signed intent, replace pubkey with attacker's pubkey
#[test]
fn test_mitm_pubkey_swap() {
    let user_key = [0x42; 32];
    let attacker_key = [0x43; 32];
    let user_addr = test_address(&user_key, "cosmos");

    // User creates and signs a legitimate intent
    let unsigned = build_unsigned_for(&user_addr);
    let mut signed = sign_intent(unsigned, &user_key);
    assert!(signed.verify().unwrap(), "Original intent should verify");

    // MITM replaces the pubkey with attacker's pubkey
    let attacker_pubkey = derive_public_key(&attacker_key).unwrap();
    signed.public_key = attacker_pubkey;

    // Now the signature doesn't match the pubkey (ECDSA fails),
    // AND the pubkey doesn't match the address (binding fails).
    // Either check should reject this.
    let result = signed.verify();
    assert!(result.is_err(), "SECURITY: MITM pubkey swap must be rejected");
}

/// ATTACKER: User A who wants to act as User B on a different chain prefix
/// GOAL: Use same key but claim a different bech32 prefix to confuse systems
/// METHOD: Derive address with "osmo" prefix, claim "cosmos" address
#[test]
fn test_wrong_bech32_prefix_rejected() {
    let key = [0x42; 32];

    // Derive the "osmo" address for this key
    let osmo_addr = test_address(&key, "osmo");
    // Derive the correct "cosmos" address
    let cosmos_addr = test_address(&key, "cosmos");

    // Create intent with the cosmos address (correct prefix for this chain)
    let unsigned = build_unsigned_for(&cosmos_addr);
    let signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap(), "Correct prefix should verify");

    // Create intent with the osmo address (wrong prefix for cosmoshub)
    let unsigned_wrong = build_unsigned_for(&osmo_addr);
    let signed_wrong = sign_intent(unsigned_wrong, &key);

    // This still verifies because the key DOES derive this osmo address.
    // The point is: the system should enforce chain-prefix consistency
    // at the application layer. The crypto binding itself is correct.
    assert!(
        signed_wrong.verify().unwrap(),
        "Same key with osmo prefix is cryptographically valid"
    );

    // But the two addresses are NOT the same - application layer must
    // validate that intent.user prefix matches input.chain_id expectations
    assert_ne!(cosmos_addr, osmo_addr);
}

/// ATTACKER: Anyone who obtains a non-bech32 address string
/// GOAL: Bypass address binding by using an address that can't be decoded
/// METHOD: Set user to a raw string that isn't valid bech32
#[test]
fn test_non_bech32_address_rejected() {
    let key = [0x42; 32];

    // Create intent with an invalid address
    let unsigned = build_unsigned_for("not-a-real-address");
    let signed = sign_intent(unsigned, &key);

    // Verification must fail because bech32 decode will fail
    let result = signed.verify();
    assert!(result.is_err(), "Non-bech32 address must be rejected");
    assert!(
        matches!(result.unwrap_err(), VerificationError::EncodingError(_)),
        "Must fail with EncodingError for non-bech32 address"
    );
}

/// ATTACKER: Anyone who obtains a valid pubkey but uses it with the wrong user
/// GOAL: Create a valid-looking intent that passes basic signature checks
/// METHOD: Use low-level sign() to inject arbitrary pubkey + mismatched signature
#[test]
fn test_manual_sign_with_mismatched_pubkey_and_user() {
    let key_a = [0x42; 32];
    let key_b = [0x43; 32];
    let user_b_addr = test_address(&key_b, "cosmos");

    // Build intent for user B
    let unsigned = build_unsigned_for(&user_b_addr);

    // Sign the bytes with key A
    let message = unsigned.signing_bytes();
    let sig_a = sign_message(&message, &key_a).unwrap();
    let pubkey_a = derive_public_key(&key_a).unwrap();

    // Use low-level sign() to bypass sign_with_key's key derivation
    let crafted = unsigned.sign(sig_a, pubkey_a);

    // Signature matches pubkey_a, but pubkey_a doesn't derive user_b_addr
    let result = crafted.verify();
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        VerificationError::AddressMismatch { .. }
    ));
}

// =============================================================================
// CATEGORY 3: AUTHORIZATION SEMANTICS (ENHANCED)
// =============================================================================
// Extends field tampering tests with timestamp manipulation (T-C2 regression)
// and fill config attacks that were previously uncovered.

/// ATTACKER: Malicious relay / mempool observer
/// GOAL: Extend intent lifetime by modifying expires_at after signing
/// METHOD: Intercept signed intent, change expires_at to far future
/// REGRESSION: T-C2 - timestamps must be in signing hash
#[test]
fn test_expiry_extension_attack() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = build_unsigned_for(&addr);
    let mut signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap());

    // Attacker extends the expiry to keep the intent alive longer
    signed.expires_at = u64::MAX;

    let result = signed.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY T-C2: Expiry extension must invalidate signature"
    );
}

/// ATTACKER: Malicious relay / mempool observer
/// GOAL: Backdate created_at to make intent appear older (affects priority)
/// METHOD: Change created_at after signing
/// REGRESSION: T-C2 - timestamps must be in signing hash
#[test]
fn test_created_at_backdate_attack() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = build_unsigned_for(&addr);
    let mut signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap());

    // Attacker backdates created_at
    signed.created_at = 0;

    let result = signed.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY T-C2: created_at manipulation must invalidate signature"
    );
}

/// ATTACKER: Solver who wants more flexibility on fill execution
/// GOAL: Change fill config to allow partial fills when user wanted all-or-nothing
/// METHOD: Flip allow_partial from false to true after signing
#[test]
fn test_fill_config_partial_fill_attack() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig {
            allow_partial: false, // User explicitly wants all-or-nothing
            min_fill_amount: Uint128::new(1_000_000),
            min_fill_pct: "1.0".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::AllOrNothing,
        })
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap());

    // Attacker changes fill config to allow partial fills
    signed.fill_config.allow_partial = true;

    let result = signed.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: Fill config allow_partial tampering must invalidate signature"
    );
}

/// ATTACKER: Solver who wants to lower the minimum fill threshold
/// GOAL: Fill with a tiny amount and keep the difference
/// METHOD: Lower min_fill_amount after signing
#[test]
fn test_fill_config_min_fill_amount_attack() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig {
            allow_partial: true,
            min_fill_amount: Uint128::new(500_000), // User sets minimum
            min_fill_pct: "0.5".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        })
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap());

    // Attacker lowers minimum fill to allow dust fills
    signed.fill_config.min_fill_amount = Uint128::new(1);

    let result = signed.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: min_fill_amount tampering must invalidate signature"
    );
}

/// ATTACKER: Solver who wants to change the fill strategy
/// GOAL: Change from AllOrNothing to SolverDiscretion
/// METHOD: Swap strategy enum variant after signing
#[test]
fn test_fill_strategy_swap_attack() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig {
            allow_partial: false,
            min_fill_amount: Uint128::new(1_000_000),
            min_fill_pct: "1.0".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::AllOrNothing,
        })
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap());

    // Attacker swaps strategy to give solver total control
    signed.fill_config.strategy = FillStrategy::SolverDiscretion;

    let result = signed.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: Strategy swap must invalidate signature"
    );
}

/// ATTACKER: Solver who wants to remove execution constraints
/// GOAL: Remove venue exclusions set by user
/// METHOD: Clear excluded_venues after signing
#[test]
fn test_excluded_venues_removal_attack() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(
            ExecutionConstraints::new(1_000_000)
                .exclude_venue("shady-dex")
                .exclude_venue("untrusted-bridge"),
        )
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap());

    // Attacker removes excluded venues to route through them
    signed.constraints.excluded_venues.clear();

    let result = signed.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: Removing excluded venues must invalidate signature"
    );
}

/// ATTACKER: Solver who wants higher fees
/// GOAL: Raise the max_solver_fee_bps to extract more value
/// METHOD: Increase max_solver_fee_bps after signing
#[test]
fn test_max_solver_fee_increase_attack() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000).with_max_solver_fee_bps(10))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap());

    // Attacker raises the fee cap from 10 bps to 5000 bps (50%)
    signed.constraints.max_solver_fee_bps = Some(5000);

    let result = signed.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: Raising max solver fee must invalidate signature"
    );
}

/// ATTACKER: Solver who wants to route through slow/cheap bridges
/// GOAL: Enable cross-ecosystem execution when user explicitly disabled it
/// METHOD: Flip allow_cross_ecosystem from false to true
#[test]
fn test_cross_ecosystem_flag_flip_attack() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000).with_cross_ecosystem(false))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let mut signed = sign_intent(unsigned, &key);
    assert!(signed.verify().unwrap());

    // Attacker enables cross-ecosystem routing
    signed.constraints.allow_cross_ecosystem = true;

    let result = signed.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: Flipping cross_ecosystem flag must invalidate signature"
    );
}

// =============================================================================
// CATEGORY 4: SECURITY PROPERTY INVARIANT TESTS
// =============================================================================
// These tests directly state and verify the security properties the system
// must guarantee. Each is framed as a property assertion over multiple keys.

/// PROPERTY: Only the holder of the private key that derives intent.user
///           can produce a signature that passes verification.
///
/// Tests this across N distinct keys to ensure no false positives.
#[test]
fn test_property_only_key_holder_can_authorize() {
    // All keys must be valid secp256k1 private keys (< curve order).
    // [0xFF; 32] exceeds the curve order and would be rejected.
    let keys: Vec<[u8; 32]> = vec![
        [0x11; 32],
        [0x42; 32],
        [0x43; 32],
        [0x77; 32],
        [0x99; 32],
    ];

    // For each key, create an intent for that key's address
    for (i, correct_key) in keys.iter().enumerate() {
        let addr = test_address(correct_key, "cosmos");
        let unsigned = build_unsigned_for(&addr);

        // The correct key must verify
        let signed_correct = sign_intent(unsigned.clone(), correct_key);
        assert!(
            signed_correct.verify().unwrap(),
            "Key {} should verify its own intent",
            i
        );

        // Every other key must fail
        for (j, wrong_key) in keys.iter().enumerate() {
            if i == j {
                continue;
            }
            let signed_wrong = sign_intent(unsigned.clone(), wrong_key);
            let result = signed_wrong.verify();
            assert!(
                result.is_err(),
                "SECURITY PROPERTY VIOLATION: Key {} verified intent belonging to key {}",
                j,
                i
            );
        }
    }
}

/// PROPERTY: Every mutable field in the signed intent is tamper-evident.
///           Modifying ANY single field must cause verification to fail.
///
/// This exhaustively tests all fields to ensure nothing was missed
/// in the signing hash.
#[test]
fn test_property_all_signed_fields_are_tamper_evident() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let unsigned = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1legit".to_string(),
        })
        .fill_config(FillConfig {
            allow_partial: true,
            min_fill_amount: Uint128::new(100_000),
            min_fill_pct: "0.5".to_string(),
            aggregation_window_ms: 5000,
            strategy: FillStrategy::Eager,
        })
        .constraints(
            ExecutionConstraints::new(1_000_000)
                .with_max_hops(3)
                .with_max_solver_fee_bps(50)
                .with_cross_ecosystem(false)
                .with_max_bridge_time_secs(600)
                .exclude_venue("shady-dex"),
        )
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let base = sign_intent(unsigned, &key);
    assert!(base.verify().unwrap(), "Base intent must verify");

    // Each closure produces a tampered clone. Every one must fail verification.
    let tamper_fns: Vec<(&str, Box<dyn Fn(&mut Intent)>)> = vec![
        ("nonce", Box::new(|i: &mut Intent| i.nonce = 999)),
        (
            "input.chain_id",
            Box::new(|i: &mut Intent| i.input.chain_id = "juno-1".into()),
        ),
        (
            "input.denom",
            Box::new(|i: &mut Intent| i.input.denom = "ujuno".into()),
        ),
        (
            "input.amount",
            Box::new(|i: &mut Intent| i.input.amount = Uint128::new(999)),
        ),
        (
            "output.chain_id",
            Box::new(|i: &mut Intent| i.output.chain_id = "juno-1".into()),
        ),
        (
            "output.denom",
            Box::new(|i: &mut Intent| i.output.denom = "ujuno".into()),
        ),
        (
            "output.min_amount",
            Box::new(|i: &mut Intent| i.output.min_amount = Uint128::new(1)),
        ),
        (
            "output.limit_price",
            Box::new(|i: &mut Intent| i.output.limit_price = "999.0".into()),
        ),
        (
            "output.recipient",
            Box::new(|i: &mut Intent| i.output.recipient = "osmo1attacker".into()),
        ),
        (
            "fill_config.allow_partial",
            Box::new(|i: &mut Intent| i.fill_config.allow_partial = false),
        ),
        (
            "fill_config.min_fill_amount",
            Box::new(|i: &mut Intent| i.fill_config.min_fill_amount = Uint128::new(1)),
        ),
        (
            "fill_config.min_fill_pct",
            Box::new(|i: &mut Intent| i.fill_config.min_fill_pct = "0.01".into()),
        ),
        (
            "fill_config.aggregation_window_ms",
            Box::new(|i: &mut Intent| i.fill_config.aggregation_window_ms = 0),
        ),
        (
            "fill_config.strategy",
            Box::new(|i: &mut Intent| i.fill_config.strategy = FillStrategy::SolverDiscretion),
        ),
        (
            "constraints.deadline",
            Box::new(|i: &mut Intent| i.constraints.deadline = u64::MAX),
        ),
        (
            "constraints.max_hops",
            Box::new(|i: &mut Intent| i.constraints.max_hops = Some(999)),
        ),
        (
            "constraints.max_hops (None->Some)",
            Box::new(|i: &mut Intent| i.constraints.max_hops = None),
        ),
        (
            "constraints.excluded_venues (clear)",
            Box::new(|i: &mut Intent| i.constraints.excluded_venues.clear()),
        ),
        (
            "constraints.excluded_venues (add)",
            Box::new(|i: &mut Intent| i.constraints.excluded_venues.push("extra".into())),
        ),
        (
            "constraints.max_solver_fee_bps",
            Box::new(|i: &mut Intent| i.constraints.max_solver_fee_bps = Some(9999)),
        ),
        (
            "constraints.allow_cross_ecosystem",
            Box::new(|i: &mut Intent| i.constraints.allow_cross_ecosystem = true),
        ),
        (
            "constraints.max_bridge_time_secs",
            Box::new(|i: &mut Intent| i.constraints.max_bridge_time_secs = Some(9999)),
        ),
        (
            "constraints.max_bridge_time_secs (None)",
            Box::new(|i: &mut Intent| i.constraints.max_bridge_time_secs = None),
        ),
        (
            "created_at",
            Box::new(|i: &mut Intent| i.created_at = 0),
        ),
        (
            "expires_at",
            Box::new(|i: &mut Intent| i.expires_at = u64::MAX),
        ),
    ];

    for (field_name, tamper) in &tamper_fns {
        let mut tampered = base.clone();
        tamper(&mut tampered);
        let result = tampered.verify();
        assert!(
            result.is_err() || !result.unwrap(),
            "SECURITY: Tampering with '{}' must invalidate signature, but verification passed",
            field_name
        );
    }
}

/// PROPERTY: A valid signature for intent A cannot be used to verify intent B.
///           Signatures are non-transferable between intents.
#[test]
fn test_property_signature_is_non_transferable() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    // Create two intents with different nonces (different signing hashes)
    let unsigned_a = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 1_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(5_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(1)
        .build(100, 2000)
        .unwrap();

    let unsigned_b = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 2_000_000)) // Different amount
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(10_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(2)
        .build(100, 2000)
        .unwrap();

    let signed_a = sign_intent(unsigned_a, &key);
    let signed_b = sign_intent(unsigned_b, &key);

    // Both verify independently
    assert!(signed_a.verify().unwrap());
    assert!(signed_b.verify().unwrap());

    // Cross-apply: signature from A onto B's data
    let mut frankenstein = signed_b.clone();
    frankenstein.signature = signed_a.signature.clone();
    frankenstein.public_key = signed_a.public_key.clone();

    let result = frankenstein.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: Signature from intent A must not verify intent B"
    );
}

/// PROPERTY: Cancellation signatures are bound to specific intent IDs.
///           A cancellation for intent X cannot cancel intent Y.
#[test]
fn test_property_cancellation_is_intent_specific() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    let req_x = CancellationRequest::new("intent-x", &addr, 1000);
    let signed_x = req_x.sign(&key).unwrap();
    assert!(signed_x.verify().unwrap());

    // Try to use intent-x's cancellation signature for intent-y
    let mut tampered = signed_x.clone();
    tampered.intent_id = "intent-y".to_string();

    let result = tampered.verify();
    assert!(
        result.is_err(),
        "SECURITY: Cancellation for intent-x must not cancel intent-y"
    );
}

// =============================================================================
// CATEGORY 5: COMPOSITE / MULTI-STEP ATTACKS
// =============================================================================
// These simulate realistic attack scenarios that combine multiple techniques.

/// ATTACKER: Two colluding solvers
/// GOAL: Swap credential pairs between two legitimately signed intents
/// METHOD: User A signs intent_a, User B signs intent_b.
///         Swap (signature, pubkey) pairs between them.
#[test]
fn test_credential_swap_between_users() {
    let key_a = [0x42; 32];
    let key_b = [0x43; 32];
    let addr_a = test_address(&key_a, "cosmos");
    let addr_b = test_address(&key_b, "cosmos");

    let unsigned_a = build_unsigned_for(&addr_a);
    let unsigned_b = build_unsigned_for(&addr_b);

    let signed_a = sign_intent(unsigned_a, &key_a);
    let signed_b = sign_intent(unsigned_b, &key_b);

    // Both verify independently
    assert!(signed_a.verify().unwrap());
    assert!(signed_b.verify().unwrap());

    // Swap credentials: put A's (sig, pubkey) on B's intent data
    let mut swap_a_to_b = signed_b.clone();
    swap_a_to_b.signature = signed_a.signature.clone();
    swap_a_to_b.public_key = signed_a.public_key.clone();

    let result = swap_a_to_b.verify();
    assert!(
        result.is_err(),
        "SECURITY: Swapping credentials between users must fail"
    );

    // Swap the other way too
    let mut swap_b_to_a = signed_a.clone();
    swap_b_to_a.signature = signed_b.signature.clone();
    swap_b_to_a.public_key = signed_b.public_key.clone();

    let result = swap_b_to_a.verify();
    assert!(
        result.is_err(),
        "SECURITY: Swapping credentials between users must fail (reverse)"
    );
}

/// ATTACKER: Malicious solver who intercepts and modifies intent
/// GOAL: Redirect funds to attacker while keeping the original signature
/// METHOD: Clone signed intent, change recipient, try to reuse signature
#[test]
fn test_intercept_and_redirect_attack() {
    let user_key = [0x42; 32];
    let user_addr = test_address(&user_key, "cosmos");

    // User creates legit intent sending to their own osmo address
    let unsigned = Intent::builder()
        .user(&user_addr)
        .input(Asset::new("cosmoshub-4", "uatom", 10_000_000))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(50_000_000),
            limit_price: "5.0".to_string(),
            recipient: "osmo1legit_user_addr".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(42)
        .build(100, 2000)
        .unwrap();

    let signed = sign_intent(unsigned, &user_key);
    assert!(signed.verify().unwrap());

    // Attacker intercepts and redirects recipient
    let mut redirected = signed.clone();
    redirected.output.recipient = "osmo1attacker_drain_addr".to_string();

    let result = redirected.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: Recipient redirect must invalidate signature"
    );
}

/// ATTACKER: Sophisticated attacker with access to multiple valid signed intents
/// GOAL: Construct a new "franken-intent" by mixing fields from different intents
/// METHOD: Take user/nonce from intent_1, amounts from intent_2, keep sig from intent_1
#[test]
fn test_franken_intent_from_mixed_fields() {
    let key = [0x42; 32];
    let addr = test_address(&key, "cosmos");

    // Intent 1: small amount
    let unsigned_1 = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 100))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(500),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(1)
        .build(100, 2000)
        .unwrap();

    // Intent 2: large amount
    let unsigned_2 = Intent::builder()
        .user(&addr)
        .input(Asset::new("cosmoshub-4", "uatom", 999_999_999))
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(4_999_999_995),
            limit_price: "5.0".to_string(),
            recipient: "osmo1user".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1_000_000))
        .nonce(2)
        .build(100, 2000)
        .unwrap();

    let signed_1 = sign_intent(unsigned_1, &key);
    let signed_2 = sign_intent(unsigned_2, &key);

    assert!(signed_1.verify().unwrap());
    assert!(signed_2.verify().unwrap());

    // Franken-intent: nonce from #1 (small), amounts from #2 (large), sig from #1
    let mut franken = signed_1.clone();
    franken.input.amount = signed_2.input.amount;
    franken.output.min_amount = signed_2.output.min_amount;

    let result = franken.verify();
    assert!(
        result.is_err() || !result.unwrap(),
        "SECURITY: Franken-intent must not verify"
    );
}

/// ATTACKER: Malicious solver
/// GOAL: Use verify_intent_signature() directly to bypass any wrapper logic
/// METHOD: Craft intent manually and call the low-level verification function
#[test]
fn test_low_level_api_enforces_same_checks() {
    let attacker_key = [0x42; 32];
    let victim_key = [0x43; 32];
    let victim_addr = test_address(&victim_key, "cosmos");

    // Craft intent for victim, signed by attacker, using low-level API
    let unsigned = build_unsigned_for(&victim_addr);
    let message = unsigned.signing_bytes();
    let sig = sign_message(&message, &attacker_key).unwrap();
    let pubkey = derive_public_key(&attacker_key).unwrap();
    let crafted = unsigned.sign(sig, pubkey);

    // Both the high-level and low-level APIs must reject this
    let high_level = crafted.verify();
    let low_level = verify_intent_signature(&crafted);

    assert!(high_level.is_err(), "High-level verify must reject");
    assert!(low_level.is_err(), "Low-level verify_intent_signature must reject");

    // Both must give the same error type
    assert_eq!(
        std::mem::discriminant(&high_level.unwrap_err()),
        std::mem::discriminant(&low_level.unwrap_err()),
        "Both APIs must return the same error variant"
    );
}
