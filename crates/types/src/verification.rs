use cosmwasm_std::Binary;
use k256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::Intent;

/// Errors that can occur during signature verification
#[derive(Debug, Error, PartialEq)]
pub enum VerificationError {
    #[error("invalid signature: {0}")]
    InvalidSignature(String),

    #[error("missing signature")]
    MissingSignature,

    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),

    #[error("encoding error: {0}")]
    EncodingError(String),

    #[error("signature verification failed")]
    VerificationFailed,

    #[error("address mismatch: pubkey derives {derived}, but intent claims {claimed}")]
    AddressMismatch { derived: String, claimed: String },
}

/// Derive a bech32 Cosmos address from a compressed secp256k1 public key.
///
/// Standard Cosmos address derivation:
///   compressed_pubkey -> SHA-256 -> RIPEMD-160 -> bech32(prefix, hash)
pub fn pubkey_to_bech32_address(pubkey: &[u8], prefix: &str) -> Result<String, VerificationError> {
    if pubkey.len() != 33 {
        return Err(VerificationError::InvalidPublicKey(format!(
            "expected 33-byte compressed pubkey, got {} bytes",
            pubkey.len()
        )));
    }

    let sha256_hash = Sha256::digest(pubkey);
    let ripemd_hash = Ripemd160::digest(&sha256_hash);

    let hrp = bech32::Hrp::parse(prefix).map_err(|e| {
        VerificationError::EncodingError(format!("invalid bech32 prefix '{}': {}", prefix, e))
    })?;

    bech32::encode::<bech32::Bech32>(hrp, &ripemd_hash[..]).map_err(|e| {
        VerificationError::EncodingError(format!("bech32 encoding failed: {}", e))
    })
}

/// Extract the bech32 human-readable prefix from an address.
pub(crate) fn extract_bech32_prefix(address: &str) -> Result<String, VerificationError> {
    let (hrp, _) = bech32::decode(address).map_err(|e| {
        VerificationError::EncodingError(format!(
            "cannot decode bech32 address '{}': {}",
            address, e
        ))
    })?;
    Ok(hrp.to_string())
}

/// Verify an intent's signature using secp256k1
///
/// This function:
/// 1. Extracts the signing bytes from the intent (canonical representation)
/// 2. Verifies the signature against the provided public key
/// 3. Supports standard Cosmos SDK secp256k1 signatures
///
/// # Arguments
/// * `intent` - The intent to verify
///
/// # Returns
/// * `Ok(true)` if signature is valid
/// * `Err(VerificationError)` if verification fails
pub fn verify_intent_signature(intent: &Intent) -> Result<bool, VerificationError> {
    // Check that signature exists and is not empty
    if intent.signature.is_empty() {
        return Err(VerificationError::MissingSignature);
    }

    // Check that public key exists and is not empty
    if intent.public_key.is_empty() {
        return Err(VerificationError::InvalidPublicKey(
            "public key is empty".to_string(),
        ));
    }

    // Parse the public key (33 bytes for compressed secp256k1)
    let verifying_key = VerifyingKey::from_sec1_bytes(&intent.public_key)
        .map_err(|e| VerificationError::InvalidPublicKey(e.to_string()))?;

    // Get the canonical signing bytes
    let message = intent.signing_bytes();

    // Parse the signature (64 bytes for compact representation)
    let signature = Signature::from_slice(&intent.signature)
        .map_err(|e| VerificationError::InvalidSignature(e.to_string()))?;

    // Verify the signature
    verifying_key
        .verify(&message, &signature)
        .map_err(|_e| VerificationError::VerificationFailed)?;

    // Verify pubkey derives the claimed user address (T-C4/T-C5)
    let prefix = extract_bech32_prefix(&intent.user)?;
    let derived_address = pubkey_to_bech32_address(&intent.public_key, &prefix)?;
    if derived_address != intent.user {
        return Err(VerificationError::AddressMismatch {
            derived: derived_address,
            claimed: intent.user.clone(),
        });
    }

    Ok(true)
}

/// Verify a signature with Cosmos SDK amino signing format
///
/// Amino encoding is used in legacy Cosmos SDK transactions.
/// The message is prefixed with length information before signing.
pub fn verify_amino_signature(intent: &Intent, sign_doc: &[u8]) -> Result<bool, VerificationError> {
    if intent.signature.is_empty() {
        return Err(VerificationError::MissingSignature);
    }

    if intent.public_key.is_empty() {
        return Err(VerificationError::InvalidPublicKey(
            "public key is empty".to_string(),
        ));
    }

    let verifying_key = VerifyingKey::from_sec1_bytes(&intent.public_key)
        .map_err(|e| VerificationError::InvalidPublicKey(e.to_string()))?;

    // Hash the amino sign doc
    let mut hasher = Sha256::new();
    hasher.update(sign_doc);
    let message = hasher.finalize();

    let signature = Signature::from_slice(&intent.signature)
        .map_err(|e| VerificationError::InvalidSignature(e.to_string()))?;

    verifying_key
        .verify(&message, &signature)
        .map_err(|_| VerificationError::VerificationFailed)?;

    Ok(true)
}

/// Sign a message using secp256k1 with a given private key
///
/// This is a helper function for testing and client-side signing.
///
/// # Arguments
/// * `message` - The message bytes to sign
/// * `private_key_bytes` - The 32-byte private key
///
/// # Returns
/// * Signature as Binary (64 bytes)
pub fn sign_message(message: &[u8], private_key_bytes: &[u8]) -> Result<Binary, VerificationError> {
    use k256::ecdsa::{signature::Signer, SigningKey};

    if private_key_bytes.len() != 32 {
        return Err(VerificationError::EncodingError(format!(
            "private key must be 32 bytes, got {}",
            private_key_bytes.len()
        )));
    }

    let signing_key = SigningKey::from_bytes(private_key_bytes.into())
        .map_err(|e| VerificationError::EncodingError(e.to_string()))?;

    let signature: Signature = signing_key.sign(message);

    Ok(Binary::from(signature.to_bytes().to_vec()))
}

/// Verify a signature against a message and public key
///
/// Generic signature verification that can be used for any signed message.
///
/// # Arguments
/// * `message` - The message bytes that were signed
/// * `signature` - The signature (64 bytes)
/// * `public_key` - The public key (33 bytes compressed)
///
/// # Returns
/// * `Ok(true)` if signature is valid
/// * `Ok(false)` should not happen (errors returned instead)
/// * `Err(VerificationError)` if verification fails
pub fn verify_signature(
    message: &[u8],
    signature: &Binary,
    public_key: &Binary,
) -> Result<bool, VerificationError> {
    if signature.is_empty() {
        return Err(VerificationError::MissingSignature);
    }

    if public_key.is_empty() {
        return Err(VerificationError::InvalidPublicKey(
            "public key is empty".to_string(),
        ));
    }

    let verifying_key = VerifyingKey::from_sec1_bytes(public_key)
        .map_err(|e| VerificationError::InvalidPublicKey(e.to_string()))?;

    let sig = Signature::from_slice(signature)
        .map_err(|e| VerificationError::InvalidSignature(e.to_string()))?;

    verifying_key
        .verify(message, &sig)
        .map_err(|_| VerificationError::VerificationFailed)?;

    Ok(true)
}

/// Extract the public key from a private key
///
/// # Arguments
/// * `private_key_bytes` - The 32-byte private key
///
/// # Returns
/// * Compressed public key as Binary (33 bytes)
pub fn derive_public_key(private_key_bytes: &[u8]) -> Result<Binary, VerificationError> {
    use k256::ecdsa::SigningKey;

    if private_key_bytes.len() != 32 {
        return Err(VerificationError::EncodingError(format!(
            "private key must be 32 bytes, got {}",
            private_key_bytes.len()
        )));
    }

    let signing_key = SigningKey::from_bytes(private_key_bytes.into())
        .map_err(|e| VerificationError::EncodingError(e.to_string()))?;

    let verifying_key = signing_key.verifying_key();
    let public_key_bytes = verifying_key.to_sec1_bytes();

    Ok(Binary::from(public_key_bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Asset, ExecutionConstraints, FillConfig, OutputSpec};
    use cosmwasm_std::Uint128;

    fn create_test_intent() -> Intent {
        create_test_intent_with_user("cosmos1user")
    }

    fn create_test_intent_with_user(user: &str) -> Intent {
        let intent = Intent::builder()
            .user(user)
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
                recipient: "osmo1user".to_string(),
            })
            .fill_config(FillConfig::default())
            .constraints(ExecutionConstraints::new(1000000))
            .nonce(1)
            .build(100, 2000)
            .unwrap();

        // Create unsigned intent with dummy signature
        intent.sign(Binary::default(), Binary::default())
    }

    #[test]
    fn test_sign_and_verify_roundtrip() {
        // Generate a test private key (32 bytes)
        let private_key = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];

        // Derive public key and correct address
        let public_key = derive_public_key(&private_key).unwrap();
        assert_eq!(public_key.len(), 33); // Compressed public key

        let correct_address =
            pubkey_to_bech32_address(&public_key, "cosmos").unwrap();

        // Create intent with the correct derived address
        let mut intent = create_test_intent_with_user(&correct_address);

        // Get signing bytes
        let message = intent.signing_bytes();

        // Sign the message
        let signature = sign_message(&message, &private_key).unwrap();
        assert_eq!(signature.len(), 64); // Compact signature

        // Update intent with signature and public key
        intent.signature = signature;
        intent.public_key = public_key;

        // Verify the signature (now includes address binding check)
        let result = verify_intent_signature(&intent);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }

    #[test]
    fn test_verify_fails_with_missing_signature() {
        let intent = create_test_intent();

        let result = verify_intent_signature(&intent);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VerificationError::MissingSignature);
    }

    #[test]
    fn test_verify_fails_with_invalid_signature() {
        let mut intent = create_test_intent();

        let private_key = [0x42; 32];
        let public_key = derive_public_key(&private_key).unwrap();

        // Create an invalid signature (wrong length)
        intent.signature = Binary::from(vec![0x00, 0x01, 0x02]);
        intent.public_key = public_key;

        let result = verify_intent_signature(&intent);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::InvalidSignature(_)
        ));
    }

    #[test]
    fn test_verify_fails_with_tampered_intent() {
        let mut intent = create_test_intent();

        let private_key = [0x42; 32];
        let public_key = derive_public_key(&private_key).unwrap();

        // Sign the original intent
        let message = intent.signing_bytes();
        let signature = sign_message(&message, &private_key).unwrap();

        intent.signature = signature;
        intent.public_key = public_key;

        // Tamper with the intent after signing
        intent.nonce = 999;

        let result = verify_intent_signature(&intent);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VerificationError::VerificationFailed);
    }

    #[test]
    fn test_verify_fails_with_wrong_public_key() {
        let mut intent = create_test_intent();

        let private_key1 = [0x42; 32];
        let private_key2 = [0x43; 32];

        // Sign with one key
        let message = intent.signing_bytes();
        let signature = sign_message(&message, &private_key1).unwrap();

        // But provide different public key
        let wrong_public_key = derive_public_key(&private_key2).unwrap();

        intent.signature = signature;
        intent.public_key = wrong_public_key;

        let result = verify_intent_signature(&intent);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VerificationError::VerificationFailed);
    }

    #[test]
    fn test_invalid_public_key() {
        let mut intent = create_test_intent();

        intent.signature = Binary::from(vec![0; 64]);
        intent.public_key = Binary::from(vec![0xff; 10]); // Invalid public key

        let result = verify_intent_signature(&intent);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::InvalidPublicKey(_)
        ));
    }

    #[test]
    fn test_derive_public_key_invalid_private_key() {
        let invalid_private_key = vec![0x01, 0x02]; // Too short
        let result = derive_public_key(&invalid_private_key);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::EncodingError(_)
        ));
    }

    #[test]
    fn test_sign_message_invalid_private_key() {
        let message = b"test message";
        let invalid_private_key = vec![0x01, 0x02]; // Too short
        let result = sign_message(message, &invalid_private_key);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::EncodingError(_)
        ));
    }

    // ═══════════════════════════════════════════════════════════════════
    // T-C4/T-C5 REGRESSION TESTS: Pubkey-to-Address Binding
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_pubkey_to_bech32_address_valid() {
        let private_key = [0x42u8; 32];
        let pubkey = derive_public_key(&private_key).unwrap();
        let address = pubkey_to_bech32_address(&pubkey, "cosmos").unwrap();
        assert!(address.starts_with("cosmos1"));

        // Same key, different prefix
        let osmo_address = pubkey_to_bech32_address(&pubkey, "osmo").unwrap();
        assert!(osmo_address.starts_with("osmo1"));

        // Different prefix, same hash bytes - addresses should differ only in prefix
        // but the base data should be the same
        let (_, cosmos_data) = bech32::decode(&address).unwrap();
        let (_, osmo_data) = bech32::decode(&osmo_address).unwrap();
        assert_eq!(cosmos_data, osmo_data);
    }

    #[test]
    fn test_pubkey_to_bech32_address_invalid_key_length() {
        let result = pubkey_to_bech32_address(&[0u8; 10], "cosmos");
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::InvalidPublicKey(_)
        ));
    }

    /// T-C4 Regression: Valid signature from wrong user must be rejected.
    /// An attacker signs with key A, but sets intent.user to address-of-key-B.
    #[test]
    fn test_valid_signature_wrong_user_rejected() {
        let attacker_key = [0x42u8; 32];
        let victim_key = [0x43u8; 32];

        // Derive victim's address
        let victim_pubkey = derive_public_key(&victim_key).unwrap();
        let victim_address =
            pubkey_to_bech32_address(&victim_pubkey, "cosmos").unwrap();

        // Create intent claiming to be the victim
        let mut intent = create_test_intent_with_user(&victim_address);

        // Attacker signs with their own key
        let attacker_pubkey = derive_public_key(&attacker_key).unwrap();
        let message = intent.signing_bytes();
        let signature = sign_message(&message, &attacker_key).unwrap();

        intent.signature = signature;
        intent.public_key = attacker_pubkey;

        // Verification must fail - attacker's pubkey doesn't derive victim's address
        let result = verify_intent_signature(&intent);
        assert!(result.is_err());
        match result.unwrap_err() {
            VerificationError::AddressMismatch { derived, claimed } => {
                assert_ne!(derived, claimed);
                assert_eq!(claimed, victim_address);
            }
            other => panic!(
                "SECURITY REGRESSION T-C4: Expected AddressMismatch, got: {:?}",
                other
            ),
        }
    }

    /// T-C4 Regression: Valid signature with correct user must be accepted.
    #[test]
    fn test_valid_signature_correct_user_accepted() {
        let private_key = [0x42u8; 32];
        let pubkey = derive_public_key(&private_key).unwrap();
        let correct_address =
            pubkey_to_bech32_address(&pubkey, "cosmos").unwrap();

        let mut intent = create_test_intent_with_user(&correct_address);
        let message = intent.signing_bytes();
        let signature = sign_message(&message, &private_key).unwrap();

        intent.signature = signature;
        intent.public_key = pubkey;

        let result = verify_intent_signature(&intent);
        assert!(
            result.is_ok(),
            "Valid signature with correct user should succeed: {:?}",
            result.unwrap_err()
        );
    }

    /// T-C5 Regression: Cancellation with wrong user must be rejected.
    #[test]
    fn test_cancellation_wrong_user_rejected() {
        use crate::CancellationRequest;

        let attacker_key = [0x42u8; 32];
        let victim_key = [0x43u8; 32];

        let victim_pubkey = derive_public_key(&victim_key).unwrap();
        let victim_address =
            pubkey_to_bech32_address(&victim_pubkey, "cosmos").unwrap();

        // Attacker creates cancellation claiming to be victim
        let req = CancellationRequest::new("intent-123", &victim_address, 1000);
        let signed = req.sign(&attacker_key).expect("signing should succeed");

        // Verification must fail - attacker's pubkey doesn't derive victim's address
        let result = signed.verify();
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), VerificationError::AddressMismatch { .. }),
            "SECURITY REGRESSION T-C5: Cancellation with wrong pubkey must be rejected"
        );
    }

    /// T-C5 Regression: Cancellation with correct user must be accepted.
    #[test]
    fn test_cancellation_correct_user_accepted() {
        use crate::CancellationRequest;

        let private_key = [0x42u8; 32];
        let pubkey = derive_public_key(&private_key).unwrap();
        let correct_address =
            pubkey_to_bech32_address(&pubkey, "cosmos").unwrap();

        let req = CancellationRequest::new("intent-123", &correct_address, 1000);
        let signed = req.sign(&private_key).expect("signing should succeed");

        let result = signed.verify();
        assert!(
            result.is_ok() && result.unwrap(),
            "Cancellation with correct pubkey should succeed"
        );
    }
}
