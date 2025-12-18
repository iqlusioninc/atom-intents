use cosmwasm_schema::cw_serde;
use cosmwasm_std::Binary;
use sha2::{Sha256, Digest};

use crate::{Asset, ExecutionConstraints, FillConfig, OutputSpec, Side, TradingPair, PROTOCOL_VERSION};

/// A user's expression of desired trade outcome
#[cw_serde]
pub struct Intent {
    // ═══════════════════════════════════════════════════════════════════════════
    // IDENTIFICATION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Unique identifier (hash of contents + nonce)
    pub id: String,

    /// Protocol version for compatibility
    pub version: String,

    /// Nonce for replay protection
    pub nonce: u64,

    // ═══════════════════════════════════════════════════════════════════════════
    // USER IDENTITY
    // ═══════════════════════════════════════════════════════════════════════════

    /// User's address on source chain
    pub user: String,

    // ═══════════════════════════════════════════════════════════════════════════
    // TRADE SPECIFICATION
    // ═══════════════════════════════════════════════════════════════════════════

    /// What the user is offering
    pub input: Asset,

    /// What the user wants
    pub output: OutputSpec,

    // ═══════════════════════════════════════════════════════════════════════════
    // EXECUTION CONFIGURATION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Partial fill settings
    pub fill_config: FillConfig,

    /// Execution constraints
    pub constraints: ExecutionConstraints,

    // ═══════════════════════════════════════════════════════════════════════════
    // AUTHENTICATION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Signature over canonical intent hash
    pub signature: Binary,

    /// Public key for verification
    pub public_key: Binary,

    // ═══════════════════════════════════════════════════════════════════════════
    // METADATA
    // ═══════════════════════════════════════════════════════════════════════════

    pub created_at: u64,
    pub expires_at: u64,
}

impl Intent {
    /// Create a new intent builder
    pub fn builder() -> IntentBuilder {
        IntentBuilder::default()
    }

    /// Get the trading pair for this intent (alphabetically ordered for consistency)
    pub fn pair(&self) -> TradingPair {
        // Alphabetically order to ensure buy and sell intents map to the same pair
        if self.input.denom < self.output.denom {
            TradingPair::new(&self.input.denom, &self.output.denom)
        } else {
            TradingPair::new(&self.output.denom, &self.input.denom)
        }
    }

    /// Determine if this is a buy or sell relative to the base asset
    pub fn side(&self) -> Side {
        // Convention: if selling base asset (e.g., ATOM), it's a sell
        // This is a simplification - real implementation would check against pair definition
        Side::Sell
    }

    /// Get the canonical bytes that should be signed
    ///
    /// This produces a deterministic byte representation of the intent
    /// that excludes the signature and public_key fields (which aren't
    /// known at signing time) and the id field (which is derived).
    ///
    /// The message is hashed with SHA-256 before signing.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let hash = self.signing_hash();
        hash.to_vec()
    }

    /// Compute the canonical hash for signing
    ///
    /// This hash includes all fields that define the intent's semantics
    /// but excludes authentication fields (signature, public_key, id).
    ///
    /// SECURITY: This hash MUST include ALL fields that affect execution
    /// to prevent signature bypass attacks.
    pub fn signing_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Core identification
        hasher.update(self.version.as_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(self.user.as_bytes());

        // Input asset
        hasher.update(self.input.chain_id.as_bytes());
        hasher.update(self.input.denom.as_bytes());
        hasher.update(self.input.amount.u128().to_le_bytes());

        // Output specification
        hasher.update(self.output.chain_id.as_bytes());
        hasher.update(self.output.denom.as_bytes());
        hasher.update(self.output.min_amount.u128().to_le_bytes());
        hasher.update(self.output.limit_price.as_bytes());
        hasher.update(self.output.recipient.as_bytes());

        // Fill configuration - ALL fields affect execution
        hasher.update([self.fill_config.allow_partial as u8]);
        hasher.update(self.fill_config.min_fill_amount.u128().to_le_bytes());
        hasher.update(self.fill_config.min_fill_pct.as_bytes());
        hasher.update(self.fill_config.aggregation_window_ms.to_le_bytes());

        // Fill strategy (serialize as JSON for deterministic representation)
        let strategy_json = serde_json::to_string(&self.fill_config.strategy)
            .unwrap_or_else(|_| "null".to_string());
        hasher.update(strategy_json.as_bytes());

        // Execution constraints - ALL fields
        hasher.update(self.constraints.deadline.to_le_bytes());

        // max_hops (Option<u32>)
        if let Some(max_hops) = self.constraints.max_hops {
            hasher.update([1u8]); // Some marker
            hasher.update(max_hops.to_le_bytes());
        } else {
            hasher.update([0u8]); // None marker
        }

        // excluded_venues (sorted for determinism)
        let mut excluded_venues = self.constraints.excluded_venues.clone();
        excluded_venues.sort();
        hasher.update((excluded_venues.len() as u32).to_le_bytes());
        for venue in excluded_venues {
            hasher.update(venue.as_bytes());
        }

        // max_solver_fee_bps (Option<u32>)
        if let Some(fee_bps) = self.constraints.max_solver_fee_bps {
            hasher.update([1u8]); // Some marker
            hasher.update(fee_bps.to_le_bytes());
        } else {
            hasher.update([0u8]); // None marker
        }

        // allow_cross_ecosystem
        hasher.update([self.constraints.allow_cross_ecosystem as u8]);

        // max_bridge_time_secs (Option<u64>)
        if let Some(bridge_time) = self.constraints.max_bridge_time_secs {
            hasher.update([1u8]); // Some marker
            hasher.update(bridge_time.to_le_bytes());
        } else {
            hasher.update([0u8]); // None marker
        }

        hasher.finalize().into()
    }

    /// Verify the signature on this intent
    ///
    /// Returns true if the signature is valid, false otherwise.
    /// Returns an error if the signature or public key is malformed.
    ///
    /// # Example
    /// ```ignore
    /// let intent = /* ... */;
    /// match intent.verify() {
    ///     Ok(true) => println!("Signature valid"),
    ///     Ok(false) => println!("Signature invalid"),
    ///     Err(e) => println!("Verification error: {}", e),
    /// }
    /// ```
    pub fn verify(&self) -> Result<bool, crate::verification::VerificationError> {
        crate::verification::verify_intent_signature(self)
    }

    /// Check if the intent has expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        current_time >= self.expires_at
    }
}

/// Builder for constructing intents
#[derive(Default)]
pub struct IntentBuilder {
    user: Option<String>,
    input: Option<Asset>,
    output: Option<OutputSpec>,
    fill_config: FillConfig,
    constraints: Option<ExecutionConstraints>,
    nonce: u64,
}

impl IntentBuilder {
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    pub fn input(mut self, input: Asset) -> Self {
        self.input = Some(input);
        self
    }

    pub fn output(mut self, output: OutputSpec) -> Self {
        self.output = Some(output);
        self
    }

    pub fn fill_config(mut self, config: FillConfig) -> Self {
        self.fill_config = config;
        self
    }

    pub fn constraints(mut self, constraints: ExecutionConstraints) -> Self {
        self.constraints = Some(constraints);
        self
    }

    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }

    /// Build the intent (unsigned)
    pub fn build(self, created_at: u64, expires_at: u64) -> Result<UnsignedIntent, IntentBuildError> {
        let user = self.user.ok_or(IntentBuildError::MissingUser)?;
        let input = self.input.ok_or(IntentBuildError::MissingInput)?;
        let output = self.output.ok_or(IntentBuildError::MissingOutput)?;
        let constraints = self.constraints.ok_or(IntentBuildError::MissingConstraints)?;

        Ok(UnsignedIntent {
            version: PROTOCOL_VERSION.to_string(),
            nonce: self.nonce,
            user,
            input,
            output,
            fill_config: self.fill_config,
            constraints,
            created_at,
            expires_at,
        })
    }
}

/// An intent without signature (ready to sign)
pub struct UnsignedIntent {
    pub version: String,
    pub nonce: u64,
    pub user: String,
    pub input: Asset,
    pub output: OutputSpec,
    pub fill_config: FillConfig,
    pub constraints: ExecutionConstraints,
    pub created_at: u64,
    pub expires_at: u64,
}

impl UnsignedIntent {
    /// Get the canonical bytes that should be signed
    ///
    /// This produces the same deterministic byte representation
    /// as Intent::signing_bytes(), but for the unsigned intent.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let hash = self.signing_hash();
        hash.to_vec()
    }

    /// Compute the canonical hash for signing
    ///
    /// SECURITY: This MUST match Intent::signing_hash() exactly to ensure
    /// the same bytes are signed and verified.
    fn signing_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Core identification
        hasher.update(self.version.as_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(self.user.as_bytes());

        // Input asset
        hasher.update(self.input.chain_id.as_bytes());
        hasher.update(self.input.denom.as_bytes());
        hasher.update(self.input.amount.u128().to_le_bytes());

        // Output specification
        hasher.update(self.output.chain_id.as_bytes());
        hasher.update(self.output.denom.as_bytes());
        hasher.update(self.output.min_amount.u128().to_le_bytes());
        hasher.update(self.output.limit_price.as_bytes());
        hasher.update(self.output.recipient.as_bytes());

        // Fill configuration - ALL fields affect execution
        hasher.update([self.fill_config.allow_partial as u8]);
        hasher.update(self.fill_config.min_fill_amount.u128().to_le_bytes());
        hasher.update(self.fill_config.min_fill_pct.as_bytes());
        hasher.update(self.fill_config.aggregation_window_ms.to_le_bytes());

        // Fill strategy (serialize as JSON for deterministic representation)
        let strategy_json = serde_json::to_string(&self.fill_config.strategy)
            .unwrap_or_else(|_| "null".to_string());
        hasher.update(strategy_json.as_bytes());

        // Execution constraints - ALL fields
        hasher.update(self.constraints.deadline.to_le_bytes());

        // max_hops (Option<u32>)
        if let Some(max_hops) = self.constraints.max_hops {
            hasher.update([1u8]); // Some marker
            hasher.update(max_hops.to_le_bytes());
        } else {
            hasher.update([0u8]); // None marker
        }

        // excluded_venues (sorted for determinism)
        let mut excluded_venues = self.constraints.excluded_venues.clone();
        excluded_venues.sort();
        hasher.update((excluded_venues.len() as u32).to_le_bytes());
        for venue in excluded_venues {
            hasher.update(venue.as_bytes());
        }

        // max_solver_fee_bps (Option<u32>)
        if let Some(fee_bps) = self.constraints.max_solver_fee_bps {
            hasher.update([1u8]); // Some marker
            hasher.update(fee_bps.to_le_bytes());
        } else {
            hasher.update([0u8]); // None marker
        }

        // allow_cross_ecosystem
        hasher.update([self.constraints.allow_cross_ecosystem as u8]);

        // max_bridge_time_secs (Option<u64>)
        if let Some(bridge_time) = self.constraints.max_bridge_time_secs {
            hasher.update([1u8]); // Some marker
            hasher.update(bridge_time.to_le_bytes());
        } else {
            hasher.update([0u8]); // None marker
        }

        hasher.finalize().into()
    }

    /// Sign the intent with a private key
    ///
    /// # Arguments
    /// * `private_key` - 32-byte secp256k1 private key
    ///
    /// # Returns
    /// * Signed Intent ready for submission
    ///
    /// # Example
    /// ```ignore
    /// let unsigned = Intent::builder()
    ///     .user("cosmos1user")
    ///     // ... other fields ...
    ///     .build(timestamp, expiry)?;
    ///
    /// let private_key = [0x42; 32];
    /// let signed = unsigned.sign_with_key(&private_key)?;
    /// ```
    pub fn sign_with_key(self, private_key: &[u8]) -> Result<Intent, crate::verification::VerificationError> {
        use crate::verification::{sign_message, derive_public_key};

        // Get the message to sign
        let message = self.signing_bytes();

        // Sign the message
        let signature = sign_message(&message, private_key)?;

        // Derive the public key
        let public_key = derive_public_key(private_key)?;

        // Create the signed intent
        Ok(self.sign(signature, public_key))
    }

    /// Add signature and public key to create a signed intent
    ///
    /// This is a lower-level method when you already have a signature.
    /// For most use cases, prefer `sign_with_key()`.
    pub fn sign(self, signature: Binary, public_key: Binary) -> Intent {
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(self.version.as_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(self.user.as_bytes());
        let hash: [u8; 32] = hasher.finalize().into();
        let id = hex::encode(&hash[..16]);

        Intent {
            id,
            version: self.version,
            nonce: self.nonce,
            user: self.user,
            input: self.input,
            output: self.output,
            fill_config: self.fill_config,
            constraints: self.constraints,
            signature,
            public_key,
            created_at: self.created_at,
            expires_at: self.expires_at,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IntentBuildError {
    #[error("missing user address")]
    MissingUser,
    #[error("missing input asset")]
    MissingInput,
    #[error("missing output specification")]
    MissingOutput,
    #[error("missing execution constraints")]
    MissingConstraints,
}

/// Intent status in the system
#[cw_serde]
pub enum IntentStatus {
    /// Submitted but not yet processed
    Pending,

    /// Partially filled
    PartiallyFilled {
        filled_amount: cosmwasm_std::Uint128,
        remaining_amount: cosmwasm_std::Uint128,
    },

    /// Fully filled
    Filled,

    /// User accepted partial fill
    Finalized,

    /// Cancelled by user (no fills)
    Cancelled,

    /// Settlement complete
    Settled,

    /// Expired without fill
    Expired,
}
