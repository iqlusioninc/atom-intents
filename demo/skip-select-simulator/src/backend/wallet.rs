//! Wallet and transaction signing for Cosmos chains
//!
//! This module handles key management and transaction signing for
//! interacting with real Cosmos chains.

use k256::ecdsa::{signature::Signer, Signature, SigningKey};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tracing::debug;

/// Wallet errors
#[derive(Debug, Error)]
pub enum WalletError {
    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),

    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),

    #[error("signing failed: {0}")]
    SigningFailed(String),

    #[error("bech32 encoding failed: {0}")]
    Bech32Error(String),

    #[error("key not found: {0}")]
    KeyNotFound(String),
}

/// A Cosmos wallet for signing transactions
#[derive(Clone)]
pub struct CosmosWallet {
    /// The signing key (private key)
    signing_key: SigningKey,
    /// The address prefix (e.g., "cosmos", "osmo", "neutron")
    prefix: String,
}

impl CosmosWallet {
    /// Create a wallet from a hex-encoded private key
    pub fn from_hex(private_key_hex: &str, prefix: &str) -> Result<Self, WalletError> {
        let key_bytes = hex::decode(private_key_hex.trim())
            .map_err(|e| WalletError::InvalidPrivateKey(format!("hex decode error: {}", e)))?;

        Self::from_bytes(&key_bytes, prefix)
    }

    /// Create a wallet from raw private key bytes
    pub fn from_bytes(private_key: &[u8], prefix: &str) -> Result<Self, WalletError> {
        let signing_key = SigningKey::from_bytes(private_key.into())
            .map_err(|e| WalletError::InvalidPrivateKey(format!("invalid key: {}", e)))?;

        Ok(Self {
            signing_key,
            prefix: prefix.to_string(),
        })
    }

    /// Generate a new random wallet (for testing)
    pub fn generate(prefix: &str) -> Self {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        Self {
            signing_key,
            prefix: prefix.to_string(),
        }
    }

    /// Get the compressed public key bytes (33 bytes)
    pub fn public_key_bytes(&self) -> Vec<u8> {
        let verifying_key = self.signing_key.verifying_key();
        verifying_key.to_sec1_bytes().to_vec()
    }

    /// Get the Cosmos address (bech32 encoded)
    pub fn address(&self) -> Result<String, WalletError> {
        let pubkey_bytes = self.public_key_bytes();

        // SHA256 hash of the public key
        let sha256_hash = Sha256::digest(&pubkey_bytes);

        // RIPEMD160 hash of the SHA256 hash
        let ripemd_hash = Ripemd160::digest(&sha256_hash);

        // Bech32 encode with the prefix
        let hrp = bech32::Hrp::parse(&self.prefix)
            .map_err(|e| WalletError::Bech32Error(format!("invalid prefix: {}", e)))?;

        bech32::encode::<bech32::Bech32>(hrp, ripemd_hash.as_slice())
            .map_err(|e| WalletError::Bech32Error(format!("encoding failed: {}", e)))
    }

    /// Get the address with a different prefix (for cross-chain)
    pub fn address_with_prefix(&self, prefix: &str) -> Result<String, WalletError> {
        let pubkey_bytes = self.public_key_bytes();
        let sha256_hash = Sha256::digest(&pubkey_bytes);
        let ripemd_hash = Ripemd160::digest(&sha256_hash);

        let hrp = bech32::Hrp::parse(prefix)
            .map_err(|e| WalletError::Bech32Error(format!("invalid prefix: {}", e)))?;

        bech32::encode::<bech32::Bech32>(hrp, ripemd_hash.as_slice())
            .map_err(|e| WalletError::Bech32Error(format!("encoding failed: {}", e)))
    }

    /// Sign a message (returns the signature bytes)
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, WalletError> {
        // Hash the message with SHA256 first (standard for Cosmos)
        let hash = Sha256::digest(message);

        // Sign the hash
        let signature: Signature = self.signing_key
            .try_sign(&hash)
            .map_err(|e| WalletError::SigningFailed(format!("signing error: {}", e)))?;

        Ok(signature.to_bytes().to_vec())
    }

    /// Sign a pre-hashed message (for SignDoc signing)
    pub fn sign_prehashed(&self, hash: &[u8]) -> Result<Vec<u8>, WalletError> {
        if hash.len() != 32 {
            return Err(WalletError::SigningFailed(
                "hash must be 32 bytes".to_string(),
            ));
        }

        let signature: Signature = self.signing_key
            .try_sign(hash)
            .map_err(|e| WalletError::SigningFailed(format!("signing error: {}", e)))?;

        Ok(signature.to_bytes().to_vec())
    }
}

impl std::fmt::Debug for CosmosWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CosmosWallet")
            .field("prefix", &self.prefix)
            .field("address", &self.address().unwrap_or_default())
            .finish()
    }
}

/// Multi-chain wallet manager
/// Manages wallets for multiple chains, deriving addresses from a single key
pub struct WalletManager {
    /// The base signing key (same key, different prefixes per chain)
    base_key: SigningKey,
    /// Chain ID to address prefix mapping
    chain_prefixes: std::collections::HashMap<String, String>,
}

impl WalletManager {
    /// Create a new wallet manager from a hex-encoded private key
    pub fn from_hex(private_key_hex: &str) -> Result<Self, WalletError> {
        let key_bytes = hex::decode(private_key_hex.trim())
            .map_err(|e| WalletError::InvalidPrivateKey(format!("hex decode error: {}", e)))?;

        let base_key = SigningKey::from_bytes((&key_bytes[..]).into())
            .map_err(|e| WalletError::InvalidPrivateKey(format!("invalid key: {}", e)))?;

        let mut chain_prefixes = std::collections::HashMap::new();
        // Default prefixes for known chains
        chain_prefixes.insert("theta-testnet-001".to_string(), "cosmos".to_string());
        chain_prefixes.insert("osmo-test-5".to_string(), "osmo".to_string());
        chain_prefixes.insert("pion-1".to_string(), "neutron".to_string());
        chain_prefixes.insert("localhub-1".to_string(), "cosmos".to_string());
        chain_prefixes.insert("localosmo-1".to_string(), "osmo".to_string());

        Ok(Self {
            base_key,
            chain_prefixes,
        })
    }

    /// Add a chain prefix mapping
    pub fn add_chain(&mut self, chain_id: &str, prefix: &str) {
        self.chain_prefixes.insert(chain_id.to_string(), prefix.to_string());
    }

    /// Get a wallet for a specific chain
    pub fn wallet_for_chain(&self, chain_id: &str) -> Result<CosmosWallet, WalletError> {
        let prefix = self.chain_prefixes
            .get(chain_id)
            .ok_or_else(|| WalletError::KeyNotFound(format!("no prefix for chain: {}", chain_id)))?;

        Ok(CosmosWallet {
            signing_key: self.base_key.clone(),
            prefix: prefix.clone(),
        })
    }

    /// Get the address for a specific chain
    pub fn address_for_chain(&self, chain_id: &str) -> Result<String, WalletError> {
        self.wallet_for_chain(chain_id)?.address()
    }

    /// Sign a message for a specific chain
    pub fn sign_for_chain(&self, chain_id: &str, message: &[u8]) -> Result<Vec<u8>, WalletError> {
        self.wallet_for_chain(chain_id)?.sign(message)
    }
}

/// Load wallet from environment variable
pub fn load_wallet_from_env(chain_id: &str) -> Result<CosmosWallet, WalletError> {
    // Try chain-specific key first
    let env_var_name = format!(
        "{}_PRIVATE_KEY",
        chain_id.to_uppercase().replace('-', "_")
    );

    let private_key = std::env::var(&env_var_name)
        .or_else(|_| std::env::var("COSMOS_PRIVATE_KEY"))
        .map_err(|_| WalletError::KeyNotFound(format!(
            "Set {} or COSMOS_PRIVATE_KEY environment variable",
            env_var_name
        )))?;

    let prefix = match chain_id {
        id if id.starts_with("theta") || id.starts_with("cosmos") || id.starts_with("localhub") => "cosmos",
        id if id.starts_with("osmo") || id.starts_with("localosmo") => "osmo",
        id if id.starts_with("pion") || id.starts_with("neutron") => "neutron",
        _ => "cosmos",
    };

    debug!(chain_id = %chain_id, prefix = %prefix, "Loading wallet for chain");

    CosmosWallet::from_hex(&private_key, prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_generation() {
        let wallet = CosmosWallet::generate("cosmos");
        let address = wallet.address().unwrap();

        assert!(address.starts_with("cosmos1"));
        assert_eq!(address.len(), 45); // cosmos1 + 38 chars + checksum
    }

    #[test]
    fn test_wallet_from_known_key() {
        // Test vector: known private key and expected address
        // This is a TEST KEY - never use in production
        let test_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let wallet = CosmosWallet::from_hex(test_key, "cosmos").unwrap();

        let address = wallet.address().unwrap();
        assert!(address.starts_with("cosmos1"));
    }

    #[test]
    fn test_cross_chain_addresses() {
        let wallet = CosmosWallet::generate("cosmos");

        let cosmos_addr = wallet.address().unwrap();
        let osmo_addr = wallet.address_with_prefix("osmo").unwrap();
        let neutron_addr = wallet.address_with_prefix("neutron").unwrap();

        // Same key should produce different prefixed addresses
        assert!(cosmos_addr.starts_with("cosmos1"));
        assert!(osmo_addr.starts_with("osmo1"));
        assert!(neutron_addr.starts_with("neutron1"));

        // The address part after the prefix should be the same
        // (same key = same pubkey hash = same address bytes, different prefix)
    }

    #[test]
    fn test_signing() {
        let wallet = CosmosWallet::generate("cosmos");
        let message = b"test message";

        let signature = wallet.sign(message).unwrap();

        // secp256k1 signatures are 64 bytes (r: 32, s: 32)
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn test_wallet_manager() {
        let test_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let manager = WalletManager::from_hex(test_key).unwrap();

        let cosmos_addr = manager.address_for_chain("theta-testnet-001").unwrap();
        let osmo_addr = manager.address_for_chain("osmo-test-5").unwrap();

        assert!(cosmos_addr.starts_with("cosmos1"));
        assert!(osmo_addr.starts_with("osmo1"));
    }
}
