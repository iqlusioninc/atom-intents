use atom_intents_types::{Intent, TradingPair};
use cosmwasm_std::Uint128;
use std::collections::HashSet;
use thiserror::Error;

/// Intent validator for checking validity before processing
pub struct IntentValidator {
    /// Supported trading pairs
    supported_pairs: HashSet<TradingPair>,
    /// Maximum allowed expiration time (seconds)
    max_expiration_secs: u64,
    /// Minimum input amount (to prevent dust)
    min_input_amount: Uint128,
}

impl IntentValidator {
    pub fn new(
        supported_pairs: HashSet<TradingPair>,
        max_expiration_secs: u64,
        min_input_amount: Uint128,
    ) -> Self {
        Self {
            supported_pairs,
            max_expiration_secs,
            min_input_amount,
        }
    }

    /// Create a validator with default settings
    pub fn default_config() -> Self {
        let mut supported_pairs = HashSet::new();
        supported_pairs.insert(TradingPair::new("uatom", "uusdc"));
        supported_pairs.insert(TradingPair::new("uosmo", "uusdc"));

        Self {
            supported_pairs,
            max_expiration_secs: 3600,            // 1 hour
            min_input_amount: Uint128::new(1000), // Minimum 1000 units
        }
    }

    /// Add a supported trading pair
    pub fn add_supported_pair(&mut self, pair: TradingPair) {
        self.supported_pairs.insert(pair);
    }

    /// Validate an intent before processing
    pub fn validate_intent(
        &self,
        intent: &Intent,
        current_time: u64,
    ) -> Result<(), ValidationError> {
        // 1. Check signature
        self.validate_signature(intent)?;

        // 2. Check expiration
        self.validate_expiration(intent, current_time)?;

        // 3. Check amounts
        self.validate_amounts(intent)?;

        // 4. Check assets (trading pair support)
        self.validate_assets(intent)?;

        // 5. Check constraints
        self.validate_constraints(intent, current_time)?;

        Ok(())
    }

    /// Validate signature
    fn validate_signature(&self, intent: &Intent) -> Result<(), ValidationError> {
        // Check signature and public key are not empty
        if intent.signature.is_empty() {
            return Err(ValidationError::MissingSignature {
                intent_id: intent.id.clone(),
            });
        }

        if intent.public_key.is_empty() {
            return Err(ValidationError::MissingPublicKey {
                intent_id: intent.id.clone(),
            });
        }

        // Verify the signature
        match intent.verify() {
            Ok(true) => Ok(()),
            Ok(false) => Err(ValidationError::InvalidSignature {
                intent_id: intent.id.clone(),
            }),
            Err(e) => Err(ValidationError::SignatureVerificationFailed {
                intent_id: intent.id.clone(),
                reason: e.to_string(),
            }),
        }
    }

    /// Validate expiration
    pub fn validate_expiration(
        &self,
        intent: &Intent,
        current_time: u64,
    ) -> Result<(), ValidationError> {
        // Check if already expired
        if intent.is_expired(current_time) {
            return Err(ValidationError::Expired {
                intent_id: intent.id.clone(),
                expires_at: intent.expires_at,
                current_time,
            });
        }

        // Check if expiration is too far in the future
        let time_until_expiry = intent.expires_at.saturating_sub(current_time);
        if time_until_expiry > self.max_expiration_secs {
            return Err(ValidationError::ExpirationTooFar {
                intent_id: intent.id.clone(),
                expires_at: intent.expires_at,
                max_allowed: current_time + self.max_expiration_secs,
            });
        }

        // Check if created_at is not in the future (clock skew protection)
        if intent.created_at > current_time + 60 {
            // Allow 60s clock skew
            return Err(ValidationError::CreatedInFuture {
                intent_id: intent.id.clone(),
                created_at: intent.created_at,
                current_time,
            });
        }

        Ok(())
    }

    /// Validate amounts
    pub fn validate_amounts(&self, intent: &Intent) -> Result<(), ValidationError> {
        // Check input amount is not zero
        if intent.input.amount.is_zero() {
            return Err(ValidationError::ZeroAmount {
                intent_id: intent.id.clone(),
                field: "input".to_string(),
            });
        }

        // Check input amount meets minimum
        if intent.input.amount < self.min_input_amount {
            return Err(ValidationError::AmountTooSmall {
                intent_id: intent.id.clone(),
                amount: intent.input.amount,
                minimum: self.min_input_amount,
            });
        }

        // Check output min_amount is not zero
        if intent.output.min_amount.is_zero() {
            return Err(ValidationError::ZeroAmount {
                intent_id: intent.id.clone(),
                field: "output.min_amount".to_string(),
            });
        }

        // Check limit price is valid
        if let Err(_) = intent.output.limit_price.parse::<f64>() {
            return Err(ValidationError::InvalidLimitPrice {
                intent_id: intent.id.clone(),
                limit_price: intent.output.limit_price.clone(),
            });
        }

        // Check limit price is positive
        let limit_price: f64 = intent.output.limit_price.parse().unwrap();
        if limit_price <= 0.0 {
            return Err(ValidationError::InvalidLimitPrice {
                intent_id: intent.id.clone(),
                limit_price: intent.output.limit_price.clone(),
            });
        }

        Ok(())
    }

    /// Validate assets (check trading pair is supported)
    pub fn validate_assets(&self, intent: &Intent) -> Result<(), ValidationError> {
        let pair = intent.pair();

        if !self.supported_pairs.contains(&pair) {
            return Err(ValidationError::UnsupportedTradingPair {
                intent_id: intent.id.clone(),
                base: pair.base.clone(),
                quote: pair.quote.clone(),
            });
        }

        // Check denoms are not the same
        if intent.input.denom == intent.output.denom {
            return Err(ValidationError::SameAssetTrading {
                intent_id: intent.id.clone(),
                denom: intent.input.denom.clone(),
            });
        }

        Ok(())
    }

    /// Validate execution constraints
    pub fn validate_constraints(
        &self,
        intent: &Intent,
        current_time: u64,
    ) -> Result<(), ValidationError> {
        // Check deadline is not in the past
        if intent.constraints.deadline < current_time {
            return Err(ValidationError::DeadlineInPast {
                intent_id: intent.id.clone(),
                deadline: intent.constraints.deadline,
                current_time,
            });
        }

        // Check deadline is before expiration
        if intent.constraints.deadline > intent.expires_at {
            return Err(ValidationError::DeadlineAfterExpiration {
                intent_id: intent.id.clone(),
                deadline: intent.constraints.deadline,
                expires_at: intent.expires_at,
            });
        }

        // Validate fill configuration
        if intent.fill_config.allow_partial {
            // If partial fills allowed, check min_fill_pct is valid
            if let Ok(min_pct) = intent.fill_config.min_fill_pct.parse::<f64>() {
                if min_pct < 0.0 || min_pct > 1.0 {
                    return Err(ValidationError::InvalidFillPercentage {
                        intent_id: intent.id.clone(),
                        percentage: intent.fill_config.min_fill_pct.clone(),
                    });
                }
            } else {
                return Err(ValidationError::InvalidFillPercentage {
                    intent_id: intent.id.clone(),
                    percentage: intent.fill_config.min_fill_pct.clone(),
                });
            }

            // Check min_fill_amount is not greater than input amount
            if intent.fill_config.min_fill_amount > intent.input.amount {
                return Err(ValidationError::MinFillExceedsInput {
                    intent_id: intent.id.clone(),
                    min_fill: intent.fill_config.min_fill_amount,
                    input: intent.input.amount,
                });
            }
        }

        Ok(())
    }
}

/// Validation errors with detailed reasons
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("intent {intent_id}: missing signature")]
    MissingSignature { intent_id: String },

    #[error("intent {intent_id}: missing public key")]
    MissingPublicKey { intent_id: String },

    #[error("intent {intent_id}: invalid signature")]
    InvalidSignature { intent_id: String },

    #[error("intent {intent_id}: signature verification failed: {reason}")]
    SignatureVerificationFailed { intent_id: String, reason: String },

    #[error("intent {intent_id}: expired at {expires_at}, current time {current_time}")]
    Expired {
        intent_id: String,
        expires_at: u64,
        current_time: u64,
    },

    #[error("intent {intent_id}: expiration {expires_at} too far in future, max {max_allowed}")]
    ExpirationTooFar {
        intent_id: String,
        expires_at: u64,
        max_allowed: u64,
    },

    #[error("intent {intent_id}: created in future at {created_at}, current time {current_time}")]
    CreatedInFuture {
        intent_id: String,
        created_at: u64,
        current_time: u64,
    },

    #[error("intent {intent_id}: zero amount in field {field}")]
    ZeroAmount { intent_id: String, field: String },

    #[error("intent {intent_id}: amount {amount} below minimum {minimum}")]
    AmountTooSmall {
        intent_id: String,
        amount: Uint128,
        minimum: Uint128,
    },

    #[error("intent {intent_id}: invalid limit price {limit_price}")]
    InvalidLimitPrice {
        intent_id: String,
        limit_price: String,
    },

    #[error("intent {intent_id}: unsupported trading pair {base}/{quote}")]
    UnsupportedTradingPair {
        intent_id: String,
        base: String,
        quote: String,
    },

    #[error("intent {intent_id}: same asset trading not allowed: {denom}")]
    SameAssetTrading { intent_id: String, denom: String },

    #[error("intent {intent_id}: deadline {deadline} is in past, current time {current_time}")]
    DeadlineInPast {
        intent_id: String,
        deadline: u64,
        current_time: u64,
    },

    #[error("intent {intent_id}: deadline {deadline} after expiration {expires_at}")]
    DeadlineAfterExpiration {
        intent_id: String,
        deadline: u64,
        expires_at: u64,
    },

    #[error("intent {intent_id}: invalid fill percentage {percentage}")]
    InvalidFillPercentage {
        intent_id: String,
        percentage: String,
    },

    #[error("intent {intent_id}: min fill amount {min_fill} exceeds input {input}")]
    MinFillExceedsInput {
        intent_id: String,
        min_fill: Uint128,
        input: Uint128,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{Asset, ExecutionConstraints, FillConfig, FillStrategy, OutputSpec};
    use cosmwasm_std::Binary;

    fn make_test_intent(
        id: &str,
        input_amount: u128,
        min_output: u128,
        created_at: u64,
        expires_at: u64,
    ) -> Intent {
        Intent {
            id: id.to_string(),
            version: "1.0".to_string(),
            nonce: 1,
            user: "cosmos1user".to_string(),
            input: Asset::new("cosmoshub-4", "uatom", input_amount),
            output: OutputSpec {
                chain_id: "noble-1".to_string(),
                denom: "uusdc".to_string(),
                min_amount: Uint128::new(min_output),
                limit_price: "10.0".to_string(),
                recipient: "noble1user".to_string(),
            },
            fill_config: FillConfig {
                allow_partial: true,
                min_fill_amount: Uint128::zero(),
                min_fill_pct: "0.1".to_string(),
                aggregation_window_ms: 5000,
                strategy: FillStrategy::Eager,
            },
            constraints: ExecutionConstraints::new(expires_at),
            signature: Binary::from(vec![1, 2, 3]), // Mock signature
            public_key: Binary::from(vec![4, 5, 6]), // Mock pubkey
            created_at,
            expires_at,
        }
    }

    #[test]
    fn test_validator_creation() {
        let validator = IntentValidator::default_config();
        assert!(validator.supported_pairs.len() >= 2);
    }

    #[test]
    fn test_validate_zero_amount() {
        let validator = IntentValidator::default_config();
        let intent = make_test_intent("test-1", 0, 100, 1000, 2000);

        let result = validator.validate_amounts(&intent);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::ZeroAmount { .. }
        ));
    }

    #[test]
    fn test_validate_amount_too_small() {
        let validator = IntentValidator::default_config();
        let intent = make_test_intent("test-1", 500, 100, 1000, 2000); // Below minimum 1000

        let result = validator.validate_amounts(&intent);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::AmountTooSmall { .. }
        ));
    }

    #[test]
    fn test_validate_expired_intent() {
        let validator = IntentValidator::default_config();
        let intent = make_test_intent("test-1", 1_000_000, 10_000_000, 1000, 2000);

        let result = validator.validate_expiration(&intent, 3000); // Current time after expiry
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::Expired { .. }
        ));
    }

    #[test]
    fn test_validate_expiration_too_far() {
        let validator = IntentValidator::default_config();
        let current_time = 1000;
        let intent = make_test_intent(
            "test-1",
            1_000_000,
            10_000_000,
            current_time,
            current_time + 7200,
        ); // 2 hours

        let result = validator.validate_expiration(&intent, current_time);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::ExpirationTooFar { .. }
        ));
    }

    #[test]
    fn test_validate_created_in_future() {
        let validator = IntentValidator::default_config();
        let current_time = 1000;
        let intent = make_test_intent(
            "test-1",
            1_000_000,
            10_000_000,
            current_time + 1000,
            current_time + 2000,
        );

        let result = validator.validate_expiration(&intent, current_time);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::CreatedInFuture { .. }
        ));
    }

    #[test]
    fn test_validate_unsupported_pair() {
        let validator = IntentValidator::default_config();
        let mut intent = make_test_intent("test-1", 1_000_000, 10_000_000, 1000, 2000);
        intent.input.denom = "uunsupported".to_string();

        let result = validator.validate_assets(&intent);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::UnsupportedTradingPair { .. }
        ));
    }

    #[test]
    fn test_validate_same_asset_trading() {
        use std::collections::HashSet;
        let mut pairs = HashSet::new();
        pairs.insert(TradingPair::new("uatom", "uatom"));
        let validator = IntentValidator::new(pairs, 3600, Uint128::new(1000));

        let mut intent = make_test_intent("test-1", 1_000_000, 10_000_000, 1000, 2000);
        intent.output.denom = "uatom".to_string(); // Same as input

        let result = validator.validate_assets(&intent);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::SameAssetTrading { .. }
        ));
    }

    #[test]
    fn test_validate_deadline_in_past() {
        let validator = IntentValidator::default_config();
        let mut intent = make_test_intent("test-1", 1_000_000, 10_000_000, 1000, 3000);
        intent.constraints.deadline = 500; // Before current time

        let result = validator.validate_constraints(&intent, 1000);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::DeadlineInPast { .. }
        ));
    }

    #[test]
    fn test_validate_deadline_after_expiration() {
        let validator = IntentValidator::default_config();
        let mut intent = make_test_intent("test-1", 1_000_000, 10_000_000, 1000, 2000);
        intent.constraints.deadline = 3000; // After expiration

        let result = validator.validate_constraints(&intent, 1000);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::DeadlineAfterExpiration { .. }
        ));
    }

    #[test]
    fn test_validate_invalid_fill_percentage() {
        let validator = IntentValidator::default_config();
        let mut intent = make_test_intent("test-1", 1_000_000, 10_000_000, 1000, 2000);
        intent.fill_config.min_fill_pct = "1.5".to_string(); // > 1.0

        let result = validator.validate_constraints(&intent, 1000);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::InvalidFillPercentage { .. }
        ));
    }

    #[test]
    fn test_validate_min_fill_exceeds_input() {
        let validator = IntentValidator::default_config();
        let mut intent = make_test_intent("test-1", 1_000_000, 10_000_000, 1000, 2000);
        intent.fill_config.min_fill_amount = Uint128::new(2_000_000); // Greater than input

        let result = validator.validate_constraints(&intent, 1000);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::MinFillExceedsInput { .. }
        ));
    }

    #[test]
    fn test_add_supported_pair() {
        let mut validator = IntentValidator::default_config();
        let new_pair = TradingPair::new("uatom", "uosmo");

        validator.add_supported_pair(new_pair.clone());
        assert!(validator.supported_pairs.contains(&new_pair));
    }
}
