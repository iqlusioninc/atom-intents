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

    /// Get the trading pair for this intent
    pub fn pair(&self) -> TradingPair {
        TradingPair::new(&self.input.denom, &self.output.denom)
    }

    /// Determine if this is a buy or sell relative to the base asset
    pub fn side(&self) -> Side {
        // Convention: if selling base asset (e.g., ATOM), it's a sell
        // This is a simplification - real implementation would check against pair definition
        Side::Sell
    }

    /// Compute the canonical hash for signing
    pub fn signing_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.version.as_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(self.user.as_bytes());
        hasher.update(self.input.chain_id.as_bytes());
        hasher.update(self.input.denom.as_bytes());
        hasher.update(self.input.amount.u128().to_le_bytes());
        hasher.update(self.output.chain_id.as_bytes());
        hasher.update(self.output.denom.as_bytes());
        hasher.update(self.output.min_amount.u128().to_le_bytes());
        hasher.update(self.output.limit_price.as_bytes());
        hasher.update(self.output.recipient.as_bytes());
        hasher.update(self.constraints.deadline.to_le_bytes());
        hasher.finalize().into()
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
    /// Add signature to create a signed intent
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
