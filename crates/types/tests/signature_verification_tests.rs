use atom_intents_types::{
    derive_public_key, sign_message, verify_intent_signature, Asset, ExecutionConstraints,
    FillConfig, Intent, OutputSpec, VerificationError,
};
use cosmwasm_std::Uint128;

/// Create a test intent with standard test data
fn create_test_intent_unsigned(
) -> Result<atom_intents_types::UnsignedIntent, atom_intents_types::IntentBuildError> {
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
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(1000000))
        .nonce(42)
        .build(100, 2000)
}

#[test]
fn test_complete_signing_and_verification_flow() {
    // Create an unsigned intent
    let unsigned = create_test_intent_unsigned().unwrap();

    // Use a test private key
    let private_key = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];

    // Sign the intent
    let signed = unsigned.sign_with_key(&private_key).unwrap();

    // Verify the signature
    assert!(signed.verify().is_ok());
    assert_eq!(signed.verify().unwrap(), true);

    // Verify signature is not empty
    assert!(!signed.signature.is_empty());
    assert_eq!(signed.signature.len(), 64); // Compact ECDSA signature

    // Verify public key is not empty
    assert!(!signed.public_key.is_empty());
    assert_eq!(signed.public_key.len(), 33); // Compressed public key
}

#[test]
fn test_signature_verification_rejects_tampered_intent() {
    let unsigned = create_test_intent_unsigned().unwrap();
    let private_key = [0x42; 32];

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Tamper with the intent after signing
    signed.nonce = 999;

    // Verification should fail
    let result = signed.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VerificationError::VerificationFailed);
}

#[test]
fn test_signature_verification_rejects_different_input_amount() {
    let unsigned = create_test_intent_unsigned().unwrap();
    let private_key = [0x42; 32];

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Tamper with the input amount
    signed.input.amount = Uint128::new(9999999);

    // Verification should fail
    let result = signed.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VerificationError::VerificationFailed);
}

#[test]
fn test_signature_verification_rejects_different_output() {
    let unsigned = create_test_intent_unsigned().unwrap();
    let private_key = [0x42; 32];

    let mut signed = unsigned.sign_with_key(&private_key).unwrap();

    // Tamper with the output recipient
    signed.output.recipient = "osmo1attacker".to_string();

    // Verification should fail
    let result = signed.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VerificationError::VerificationFailed);
}

#[test]
fn test_signature_verification_accepts_unchanged_metadata() {
    let unsigned = create_test_intent_unsigned().unwrap();
    let private_key = [0x42; 32];

    let signed = unsigned.sign_with_key(&private_key).unwrap();

    // The ID is derived from a subset of fields
    // Verification should succeed because we haven't changed signed fields

    // Verification should still succeed because we haven't changed signed fields
    let result = signed.verify();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), true);
}

#[test]
fn test_multiple_intents_with_same_key() {
    let private_key = [0x42; 32];

    // Create two different intents
    let unsigned1 = create_test_intent_unsigned().unwrap();

    let mut unsigned2 = create_test_intent_unsigned().unwrap();
    unsigned2.nonce = 100; // Different nonce

    // Sign both with the same key
    let signed1 = unsigned1.sign_with_key(&private_key).unwrap();
    let signed2 = unsigned2.sign_with_key(&private_key).unwrap();

    // Both should verify successfully
    assert!(signed1.verify().unwrap());
    assert!(signed2.verify().unwrap());

    // They should have the same public key
    assert_eq!(signed1.public_key, signed2.public_key);

    // But different signatures (because content is different)
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_different_keys_produce_different_signatures() {
    let private_key1 = [0x42; 32];
    let private_key2 = [0x43; 32];

    let unsigned1 = create_test_intent_unsigned().unwrap();
    let unsigned2 = create_test_intent_unsigned().unwrap();

    let signed1 = unsigned1.sign_with_key(&private_key1).unwrap();
    let signed2 = unsigned2.sign_with_key(&private_key2).unwrap();

    // Both should verify
    assert!(signed1.verify().unwrap());
    assert!(signed2.verify().unwrap());

    // Different public keys
    assert_ne!(signed1.public_key, signed2.public_key);

    // Different signatures
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_replay_attack_prevention_with_nonce() {
    let private_key = [0x42; 32];

    let unsigned1 = create_test_intent_unsigned().unwrap();
    let mut unsigned2 = create_test_intent_unsigned().unwrap();
    unsigned2.nonce = 43; // Different nonce

    let signed1 = unsigned1.sign_with_key(&private_key).unwrap();
    let signed2 = unsigned2.sign_with_key(&private_key).unwrap();

    // Both verify independently
    assert!(signed1.verify().unwrap());
    assert!(signed2.verify().unwrap());

    // Signatures are different due to different nonces
    assert_ne!(signed1.signature, signed2.signature);
}

#[test]
fn test_signing_bytes_are_deterministic() {
    let unsigned1 = create_test_intent_unsigned().unwrap();
    let unsigned2 = create_test_intent_unsigned().unwrap();

    // Same intent data should produce same signing bytes
    assert_eq!(unsigned1.signing_bytes(), unsigned2.signing_bytes());
}

#[test]
fn test_signing_bytes_change_with_content() {
    let unsigned1 = create_test_intent_unsigned().unwrap();
    let mut unsigned2 = create_test_intent_unsigned().unwrap();
    unsigned2.nonce = 999;

    // Different intent data should produce different signing bytes
    assert_ne!(unsigned1.signing_bytes(), unsigned2.signing_bytes());
}

#[test]
fn test_manual_signature_verification_low_level() {
    let unsigned = create_test_intent_unsigned().unwrap();
    let private_key = [0x42; 32];

    // Get signing bytes and public key manually
    let message = unsigned.signing_bytes();
    let signature = sign_message(&message, &private_key).unwrap();
    let public_key = derive_public_key(&private_key).unwrap();

    // Sign using the low-level API
    let signed = unsigned.sign(signature, public_key);

    // Verify
    assert!(signed.verify().unwrap());
}

#[test]
fn test_intent_with_different_chains() {
    let unsigned = Intent::builder()
        .user("cosmos1user123")
        .input(Asset {
            chain_id: "juno-1".to_string(),
            denom: "ujuno".to_string(),
            amount: Uint128::new(2000000),
        })
        .output(OutputSpec {
            chain_id: "stargaze-1".to_string(),
            denom: "ustars".to_string(),
            min_amount: Uint128::new(10000000),
            limit_price: "5.0".to_string(),
            recipient: "stars1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(2000000))
        .nonce(1)
        .build(200, 3000)
        .unwrap();

    let private_key = [0x55; 32];
    let signed = unsigned.sign_with_key(&private_key).unwrap();

    assert!(signed.verify().unwrap());
}

#[test]
fn test_large_amounts() {
    let unsigned = Intent::builder()
        .user("cosmos1user123")
        .input(Asset {
            chain_id: "cosmoshub-4".to_string(),
            denom: "uatom".to_string(),
            amount: Uint128::new(u128::MAX), // Maximum amount
        })
        .output(OutputSpec {
            chain_id: "osmosis-1".to_string(),
            denom: "uosmo".to_string(),
            min_amount: Uint128::new(u128::MAX),
            limit_price: "1.0".to_string(),
            recipient: "osmo1user123".to_string(),
        })
        .fill_config(FillConfig::default())
        .constraints(ExecutionConstraints::new(u64::MAX))
        .nonce(u64::MAX)
        .build(u64::MAX, u64::MAX)
        .unwrap();

    let private_key = [0x77; 32];
    let signed = unsigned.sign_with_key(&private_key).unwrap();

    assert!(signed.verify().unwrap());
}

#[test]
fn test_verify_directly_without_helper() {
    let unsigned = create_test_intent_unsigned().unwrap();
    let private_key = [0x42; 32];
    let signed = unsigned.sign_with_key(&private_key).unwrap();

    // Verify using the module-level function
    let result = verify_intent_signature(&signed);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), true);
}

#[test]
fn test_cross_intent_signature_substitution_fails() {
    let private_key1 = [0x42; 32];
    let private_key2 = [0x43; 32];

    let unsigned1 = create_test_intent_unsigned().unwrap();
    let unsigned2 = create_test_intent_unsigned().unwrap();

    let signed1 = unsigned1.sign_with_key(&private_key1).unwrap();
    let signed2 = unsigned2.sign_with_key(&private_key2).unwrap();

    // Try to substitute signature from intent2 into intent1
    let mut tampered = signed1.clone();
    tampered.signature = signed2.signature.clone();

    // Verification should fail
    let result = tampered.verify();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VerificationError::VerificationFailed);
}
