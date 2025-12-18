use cosmwasm_schema::cw_serde;
use cosmwasm_std::Binary;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

/// SECURITY FIX (5.8): Intent cancellation registry
///
/// Allows users to cancel intents that haven't been matched yet.
/// Cancellation must be signed by the same key that signed the intent.

/// Cancellation request submitted by user
#[cw_serde]
pub struct CancellationRequest {
    /// ID of intent to cancel
    pub intent_id: String,

    /// User's address (must match intent.user)
    pub user: String,

    /// Timestamp of cancellation request
    pub cancelled_at: u64,

    /// Signature proving user authorization
    pub signature: Binary,

    /// Public key for verification
    pub public_key: Binary,
}

impl CancellationRequest {
    /// Create a new cancellation request (unsigned)
    pub fn new(intent_id: impl Into<String>, user: impl Into<String>, cancelled_at: u64) -> Self {
        Self {
            intent_id: intent_id.into(),
            user: user.into(),
            cancelled_at,
            signature: Binary::default(),
            public_key: Binary::default(),
        }
    }

    /// Get the bytes to sign for this cancellation request
    pub fn signing_bytes(&self) -> Vec<u8> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(b"CANCEL:");
        hasher.update(self.intent_id.as_bytes());
        hasher.update(b":");
        hasher.update(self.user.as_bytes());
        hasher.update(b":");
        hasher.update(self.cancelled_at.to_le_bytes());

        hasher.finalize().to_vec()
    }

    /// Sign the cancellation request
    pub fn sign(
        mut self,
        private_key: &[u8],
    ) -> Result<Self, crate::verification::VerificationError> {
        use crate::verification::{derive_public_key, sign_message};

        let message = self.signing_bytes();
        self.signature = sign_message(&message, private_key)?;
        self.public_key = derive_public_key(private_key)?;

        Ok(self)
    }

    /// Verify the cancellation signature
    pub fn verify(&self) -> Result<bool, crate::verification::VerificationError> {
        use crate::verification::verify_signature;

        let message = self.signing_bytes();
        verify_signature(&message, &self.signature, &self.public_key)
    }
}

/// In-memory cancellation registry for the orchestrator
///
/// Tracks cancelled intent IDs to prevent matching cancelled intents.
#[derive(Clone, Default)]
pub struct CancellationRegistry {
    /// Set of cancelled intent IDs
    cancelled: Arc<RwLock<HashSet<String>>>,
}

impl CancellationRegistry {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Register a cancellation (after verifying signature)
    pub fn register(&self, intent_id: &str) -> bool {
        if let Ok(mut cancelled) = self.cancelled.write() {
            cancelled.insert(intent_id.to_string())
        } else {
            false
        }
    }

    /// Check if an intent has been cancelled
    pub fn is_cancelled(&self, intent_id: &str) -> bool {
        self.cancelled
            .read()
            .ok()
            .map(|c| c.contains(intent_id))
            .unwrap_or(false)
    }

    /// Remove old cancellations (cleanup)
    /// In practice, would need timestamp tracking to clean up expired cancellations
    pub fn remove(&self, intent_id: &str) -> bool {
        if let Ok(mut cancelled) = self.cancelled.write() {
            cancelled.remove(intent_id)
        } else {
            false
        }
    }

    /// Get count of cancelled intents
    pub fn count(&self) -> usize {
        self.cancelled.read().ok().map(|c| c.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cancellation_request_signing_bytes() {
        let req = CancellationRequest::new("intent-123", "cosmos1user", 1000);
        let bytes = req.signing_bytes();

        // Should be deterministic
        let bytes2 = req.signing_bytes();
        assert_eq!(bytes, bytes2);

        // Should be 32 bytes (SHA256)
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn test_cancellation_registry() {
        let registry = CancellationRegistry::new();

        assert!(!registry.is_cancelled("intent-1"));
        assert_eq!(registry.count(), 0);

        // Register cancellation
        assert!(registry.register("intent-1"));
        assert!(registry.is_cancelled("intent-1"));
        assert_eq!(registry.count(), 1);

        // Duplicate registration returns false (already exists)
        assert!(!registry.register("intent-1"));
        assert_eq!(registry.count(), 1);

        // Remove
        assert!(registry.remove("intent-1"));
        assert!(!registry.is_cancelled("intent-1"));
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_cancellation_sign_and_verify() {
        let private_key = [0x42u8; 32];

        let req = CancellationRequest::new("intent-123", "cosmos1user", 1000);
        let signed = req.sign(&private_key).expect("signing should succeed");

        // Should verify successfully
        assert!(signed.verify().expect("verify should not error"));
    }

    #[test]
    fn test_cancellation_tampered_fails_verify() {
        let private_key = [0x42u8; 32];

        let req = CancellationRequest::new("intent-123", "cosmos1user", 1000);
        let mut signed = req.sign(&private_key).expect("signing should succeed");

        // Tamper with the intent_id
        signed.intent_id = "intent-TAMPERED".to_string();

        // Should fail verification with VerificationFailed error
        let result = signed.verify();
        assert!(result.is_err(), "tampered request should fail verification");
        assert!(matches!(
            result.unwrap_err(),
            crate::verification::VerificationError::VerificationFailed
        ));
    }
}
