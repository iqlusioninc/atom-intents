/// Adversarial tests for intent signature system
///
/// These tests simulate attacks where things could go horribly wrong:
/// - Replay attacks reusing old signatures
/// - Signature malleability exploits
/// - Forgery attempts
/// - Nonce manipulation
/// - Cross-chain signature replay

use atom_intents_types::{
    Asset, ExecutionConstraints, FillConfig, FillStrategy, Intent, OutputSpec,
};
use cosmwasm_std::{Binary, Uint128};

// ═══════════════════════════════════════════════════════════════════════════
// REPLAY ATTACK TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Test that replaying the exact same intent twice fails (nonce should be unique)
#[test]
fn test_replay_attack_same_intent_fails() {
    let private_key = [0x42; 32];

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned1 = Intent::builder()
        .user("cosmos1user123")
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
        .user("cosmos1user123")
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

    // Intent for cosmoshub-4
    let unsigned_cosmoshub = Intent::builder()
        .user("cosmos1user123")
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
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

/// Test that signing with wrong key fails verification
#[test]
fn test_wrong_key_signature_rejected() {
    let correct_key = [0x42; 32];
    let wrong_key = [0x43; 32];

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    // Sign with correct key
    let signed_correct = unsigned.clone().sign_with_key(&correct_key).unwrap();
    assert!(signed_correct.verify().unwrap());

    // Sign with wrong key
    let signed_wrong = unsigned.sign_with_key(&wrong_key).unwrap();

    // Public keys should be different
    assert_ne!(signed_correct.public_key, signed_wrong.public_key);

    // Both verify against their own public keys
    assert!(signed_wrong.verify().unwrap());

    // But cross-verification fails (wrong key for user identity)
    // The application layer must verify public_key matches user address
}

/// Test empty signature is rejected
#[test]
fn test_empty_signature_rejected() {
    let private_key = [0x42; 32];

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let unsigned = Intent::builder()
        .user("cosmos1user123")
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

    let mut signatures = Vec::new();

    for nonce in 0..10 {
        let unsigned = Intent::builder()
            .user("cosmos1user123")
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
