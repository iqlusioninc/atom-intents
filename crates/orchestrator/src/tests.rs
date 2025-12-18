//! Simple unit tests for the orchestrator crate
//! Integration tests require complex mocking and are omitted for initial implementation

#[cfg(test)]
mod unit_tests {
    use crate::{
        BatchResult, ExecutionResult, ExecutionStage, IntentStatus, IntentValidator,
        OrchestratorConfig, ValidationError,
    };
    use atom_intents_types::{
        Asset, ExecutionConstraints, FillConfig, FillStrategy, Intent, OutputSpec, TradingPair,
    };
    use cosmwasm_std::{Binary, Uint128};
    use rust_decimal::Decimal;
    use std::collections::HashSet;

    fn make_test_intent(
        id: &str,
        input_amount: u128,
        min_output: u128,
        allow_partial: bool,
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
                allow_partial,
                min_fill_amount: Uint128::new(input_amount / 2),
                min_fill_pct: "0.5".to_string(),
                aggregation_window_ms: 5000,
                strategy: FillStrategy::Eager,
            },
            constraints: ExecutionConstraints::new(9999999999),
            signature: Binary::from(vec![1, 2, 3]),
            public_key: Binary::from(vec![4, 5, 6]),
            created_at: 1000,
            expires_at: 9999999999,
        }
    }

    // ==================== Configuration Tests ====================

    #[test]
    fn test_orchestrator_config_default() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.batch_interval_ms, 5000);
        assert_eq!(config.max_batch_size, 100);
        assert!(config.auto_recovery_enabled);
    }

    #[test]
    fn test_orchestrator_config_with_recovery() {
        let config = OrchestratorConfig::default().with_recovery(false);
        assert!(!config.auto_recovery_enabled);
    }

    // ==================== Execution Result Tests ====================

    #[test]
    fn test_execution_result_creation() {
        let result = ExecutionResult {
            intent_id: "intent-1".to_string(),
            input_amount: Uint128::new(1_000_000),
            output_amount: Uint128::new(10_000_000),
            execution_price: Decimal::TEN,
            solver_id: Some("solver-1".to_string()),
            settlement_id: "settlement-1".to_string(),
            completed_at: 12345,
        };

        assert_eq!(result.intent_id, "intent-1");
        assert_eq!(result.execution_price, Decimal::TEN);
    }

    #[test]
    fn test_batch_result_empty() {
        let result = BatchResult {
            results: Vec::new(),
            internal_crosses: 0,
            solver_fills: 0,
            failed: Vec::new(),
        };

        assert_eq!(result.results.len(), 0);
        assert_eq!(result.internal_crosses, 0);
    }

    // ==================== Execution Stage Tests ====================

    #[test]
    fn test_execution_stage_equality() {
        assert_eq!(ExecutionStage::Validating, ExecutionStage::Validating);
        assert_ne!(ExecutionStage::Validating, ExecutionStage::Matching);
    }

    // ==================== Validator Tests ====================

    #[test]
    fn test_validator_creation() {
        let mut pairs = HashSet::new();
        pairs.insert(TradingPair::new("uatom", "uusdc"));
        let _validator = IntentValidator::new(pairs, 3600, Uint128::new(1000));
    }

    #[test]
    fn test_validator_default_config() {
        let mut validator = IntentValidator::default_config();
        validator.add_supported_pair(TradingPair::new("uatom", "uosmo"));
        // Validator created successfully
    }

    #[test]
    fn test_validate_zero_amount() {
        let validator = IntentValidator::default_config();
        let intent = make_test_intent("test-1", 0, 100, true);

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
        let intent = make_test_intent("test-1", 500, 100, true);

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
        let intent = make_test_intent("test-1", 1_000_000, 10_000_000, true);

        let result = validator.validate_expiration(&intent, 99999999999);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::Expired { .. }
        ));
    }

    #[test]
    fn test_validate_same_asset_trading() {
        let mut pairs = HashSet::new();
        pairs.insert(TradingPair::new("uatom", "uatom")); // Add the same-asset pair to supported list
        let validator = IntentValidator::new(pairs, 3600, Uint128::new(1000));

        let mut intent = make_test_intent("test-1", 1_000_000, 10_000_000, true);
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
        let mut intent = make_test_intent("test-1", 1_000_000, 10_000_000, true);
        intent.constraints.deadline = 500;

        let result = validator.validate_constraints(&intent, 1000);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::DeadlineInPast { .. }
        ));
    }

    #[test]
    fn test_intent_status_pending() {
        let status = IntentStatus::Pending;
        match status {
            IntentStatus::Pending => {}
            _ => panic!("Expected Pending status"),
        }
    }
}
