//! Simple integration tests to demonstrate functionality

#[cfg(test)]
mod simple_tests {
    use crate::validator::IntentValidator;
    use atom_intents_types::{Asset, ExecutionConstraints, FillConfig, FillStrategy, Intent, OutputSpec, TradingPair};
    use cosmwasm_std::{Binary, Uint128};
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn test_validator_creation() {
        let mut pairs = HashSet::new();
        pairs.insert(TradingPair::new("uatom", "uusdc"));
        let _validator = IntentValidator::new(pairs, 3600, Uint128::new(1000));
    }

    #[test]
    fn test_orchestrator_config_default() {
        use crate::OrchestratorConfig;
        let config = OrchestratorConfig::default();
        assert_eq!(config.batch_interval_ms, 5000);
        assert_eq!(config.max_batch_size, 100);
        assert!(config.auto_recovery_enabled);
    }

    #[test]
    fn test_execution_result_creation() {
        use crate::ExecutionResult;
        use rust_decimal::Decimal;

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
        use crate::BatchResult;

        let result = BatchResult {
            results: Vec::new(),
            internal_crosses: 0,
            solver_fills: 0,
            failed: Vec::new(),
        };

        assert_eq!(result.results.len(), 0);
        assert_eq!(result.internal_crosses, 0);
    }

    #[test]
    fn test_execution_stage_equality() {
        use crate::ExecutionStage;

        assert_eq!(ExecutionStage::Validating, ExecutionStage::Validating);
        assert_ne!(ExecutionStage::Validating, ExecutionStage::Matching);
    }
}
