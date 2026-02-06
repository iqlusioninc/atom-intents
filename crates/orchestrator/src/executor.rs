use atom_intents_matching_engine::MatchingEngine;
use atom_intents_settlement::{SettlementError, TimeoutConfig};
use atom_intents_solver::SolutionAggregator;
use atom_intents_types::{FillStrategy, Intent, OptimalFillPlan, Solution};
use cosmwasm_std::Uint128;
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};

use crate::validator::{IntentValidator, ValidationError};

/// Execution stage tracking
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionStage {
    Validating,
    Matching,
    SolvingForQuotes,
    SelectingExecutionPath,
    InitializingSettlement,
    LockingUserFunds,
    LockingSolverBond,
    ExecutingIbcTransfers,
    CompletingSettlement,
}

/// Execution coordinator orchestrating the full execution flow
pub struct ExecutionCoordinator {
    validator: Arc<IntentValidator>,
    matching_engine: Arc<tokio::sync::Mutex<MatchingEngine>>,
    solution_aggregator: Arc<SolutionAggregator>,
    // Store as trait object to avoid generic parameters
    settlement_manager: Arc<dyn SettlementManager>,
    timeout_config: TimeoutConfig,
}

/// Trait for settlement execution
#[async_trait::async_trait]
pub trait SettlementManager: Send + Sync {
    async fn execute_settlement(
        &self,
        intent: &Intent,
        solution: &Solution,
        current_time: u64,
    ) -> Result<atom_intents_types::Settlement, SettlementError>;
}

impl ExecutionCoordinator {
    pub fn new(
        validator: Arc<IntentValidator>,
        matching_engine: Arc<tokio::sync::Mutex<MatchingEngine>>,
        solution_aggregator: Arc<SolutionAggregator>,
        settlement_manager: Arc<dyn SettlementManager>,
        timeout_config: TimeoutConfig,
    ) -> Self {
        Self {
            validator,
            matching_engine,
            solution_aggregator,
            settlement_manager,
            timeout_config,
        }
    }

    /// Coordinate the full execution of an intent
    pub async fn coordinate_execution(
        &self,
        intent: Intent,
        current_time: u64,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        info!(intent_id = %intent.id, "Starting intent execution");

        // 1. Validate intent
        info!(intent_id = %intent.id, stage = ?ExecutionStage::Validating, "Validating intent");
        self.validator
            .validate_intent(&intent, current_time)
            .map_err(ExecutionError::Validation)?;

        // 2. Submit to matching engine
        info!(intent_id = %intent.id, stage = ?ExecutionStage::Matching, "Submitting to matching engine");
        let match_result = {
            let mut engine = self.matching_engine.lock().await;
            engine
                .process_intent(&intent, current_time)
                .map_err(|e| ExecutionError::Matching {
                    reason: e.to_string(),
                })?
        };

        // Calculate matched amount
        let matched_amount: Uint128 = match_result.fills.iter().map(|f| f.input_amount).sum();

        info!(
            intent_id = %intent.id,
            matched_amount = %matched_amount,
            "Matched amount from internal orders"
        );

        // 3. Get solver quotes for remaining amount
        info!(intent_id = %intent.id, stage = ?ExecutionStage::SolvingForQuotes, "Getting solver quotes");
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let fill_plan = self
            .solution_aggregator
            .aggregate(&intent, matched_amount, current_time)
            .await
            .map_err(|e| ExecutionError::SolverAggregation {
                reason: e.to_string(),
            })?;

        // 4. Select best execution path
        info!(intent_id = %intent.id, stage = ?ExecutionStage::SelectingExecutionPath, "Selecting execution path");
        let execution_path = self.select_execution_path(&intent, &fill_plan)?;

        match execution_path {
            ExecutionPath::FullyMatched { amount } => {
                info!(
                    intent_id = %intent.id,
                    amount = %amount,
                    "Intent fully matched internally"
                );

                Ok(ExecutionOutcome::Completed {
                    intent_id: intent.id.clone(),
                    matched_amount: amount,
                    solver_fills: Vec::new(),
                    settlement_id: None,
                })
            }
            ExecutionPath::RequiresSolver {
                matched_amount,
                solver_solutions,
            } => {
                info!(
                    intent_id = %intent.id,
                    matched_amount = %matched_amount,
                    solver_count = solver_solutions.len(),
                    "Intent requires solver execution"
                );

                // For simplicity, use the first (best) solver solution
                let best_solution =
                    solver_solutions
                        .first()
                        .ok_or_else(|| ExecutionError::NoViableSolver {
                            intent_id: intent.id.clone(),
                        })?;

                // 5. Initialize two-phase settlement
                info!(intent_id = %intent.id, stage = ?ExecutionStage::InitializingSettlement, "Initializing settlement");
                let settlement_result = self
                    .settlement_manager
                    .execute_settlement(&intent, best_solution, current_time)
                    .await
                    .map_err(|e| ExecutionError::Settlement {
                        reason: e.to_string(),
                    })?;

                // Check settlement status
                match settlement_result.status {
                    atom_intents_types::SettlementStatus::Complete => {
                        info!(
                            intent_id = %intent.id,
                            settlement_id = %settlement_result.intent_id,
                            "Settlement completed successfully"
                        );

                        Ok(ExecutionOutcome::Completed {
                            intent_id: intent.id.clone(),
                            matched_amount,
                            solver_fills: vec![SolverFillInfo {
                                solver_id: best_solution.solver_id.clone(),
                                input_amount: best_solution.fill.input_amount,
                                output_amount: best_solution.fill.output_amount,
                            }],
                            settlement_id: Some(settlement_result.intent_id),
                        })
                    }
                    atom_intents_types::SettlementStatus::TimedOut => {
                        warn!(
                            intent_id = %intent.id,
                            "Settlement timed out"
                        );

                        Ok(ExecutionOutcome::Failed {
                            intent_id: intent.id.clone(),
                            stage: ExecutionStage::ExecutingIbcTransfers,
                            error: ExecutionError::SettlementTimeout {
                                intent_id: intent.id.clone(),
                            },
                        })
                    }
                    _ => {
                        warn!(
                            intent_id = %intent.id,
                            status = ?settlement_result.status,
                            "Settlement in unexpected status"
                        );

                        Ok(ExecutionOutcome::Failed {
                            intent_id: intent.id.clone(),
                            stage: ExecutionStage::CompletingSettlement,
                            error: ExecutionError::Settlement {
                                reason: format!(
                                    "Unexpected status: {:?}",
                                    settlement_result.status
                                ),
                            },
                        })
                    }
                }
            }
        }
    }

    /// Select the best execution path
    fn select_execution_path(
        &self,
        intent: &Intent,
        fill_plan: &OptimalFillPlan,
    ) -> Result<ExecutionPath, ExecutionError> {
        // Check if fully matched without solver
        if fill_plan.selected.is_empty() && fill_plan.total_input >= intent.input.amount {
            return Ok(ExecutionPath::FullyMatched {
                amount: fill_plan.total_input,
            });
        }

        // Check if we have viable solver solutions
        if fill_plan.selected.is_empty() {
            return Err(ExecutionError::NoViableSolver {
                intent_id: intent.id.clone(),
            });
        }

        // Check if fill meets minimum requirements
        let total_filled = fill_plan.total_input;
        let mut requires_full_fill = !intent.fill_config.allow_partial;
        let mut required_pct: Option<f64> = None;

        match &intent.fill_config.strategy {
            FillStrategy::AllOrNothing => {
                requires_full_fill = true;
            }
            FillStrategy::MinimumThenEager { min_pct } => {
                let parsed: f64 = min_pct.parse().map_err(|_| ExecutionError::InvalidConfiguration {
                    reason: "Invalid strategy min_pct".to_string(),
                })?;
                required_pct = Some(parsed);
            }
            FillStrategy::Eager | FillStrategy::SolverDiscretion => {}
        }

        if requires_full_fill {
            if total_filled < intent.input.amount {
                return Err(ExecutionError::InsufficientFill {
                    intent_id: intent.id.clone(),
                    filled: total_filled,
                    minimum: intent.input.amount,
                });
            }
        } else {
            // Check minimum fill amount
            if total_filled < intent.fill_config.min_fill_amount {
                return Err(ExecutionError::InsufficientFill {
                    intent_id: intent.id.clone(),
                    filled: total_filled,
                    minimum: intent.fill_config.min_fill_amount,
                });
            }

            let base_pct: f64 = intent.fill_config.min_fill_pct.parse().map_err(|_| {
                ExecutionError::InvalidConfiguration {
                    reason: "Invalid min_fill_pct".to_string(),
                }
            })?;

            let required_pct = required_pct.map(|pct| pct.max(base_pct)).unwrap_or(base_pct);
            let actual_pct = total_filled.u128() as f64 / intent.input.amount.u128() as f64;

            if actual_pct < required_pct {
                return Err(ExecutionError::InsufficientFill {
                    intent_id: intent.id.clone(),
                    filled: total_filled,
                    minimum: Uint128::new((intent.input.amount.u128() as f64 * required_pct) as u128),
                });
            }
        }

        // Extract solver solutions
        let solver_solutions: Vec<Solution> = fill_plan
            .selected
            .iter()
            .map(|(solution, _)| solution.clone())
            .collect();

        Ok(ExecutionPath::RequiresSolver {
            matched_amount: fill_plan.total_input,
            solver_solutions,
        })
    }
}

/// Execution path decision
#[derive(Debug)]
enum ExecutionPath {
    /// Fully matched through internal order crossing
    FullyMatched { amount: Uint128 },

    /// Requires solver execution
    RequiresSolver {
        matched_amount: Uint128,
        solver_solutions: Vec<Solution>,
    },
}

/// Solver fill information
#[derive(Debug, Clone)]
pub struct SolverFillInfo {
    pub solver_id: String,
    pub input_amount: Uint128,
    pub output_amount: Uint128,
}

/// Outcome of intent execution
#[derive(Debug)]
pub enum ExecutionOutcome {
    /// Successfully completed
    Completed {
        intent_id: String,
        matched_amount: Uint128,
        solver_fills: Vec<SolverFillInfo>,
        settlement_id: Option<String>,
    },

    /// Execution failed
    Failed {
        intent_id: String,
        stage: ExecutionStage,
        error: ExecutionError,
    },
}

/// Execution errors
#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("validation failed: {0}")]
    Validation(#[from] ValidationError),

    #[error("matching failed: {reason}")]
    Matching { reason: String },

    #[error("solver aggregation failed: {reason}")]
    SolverAggregation { reason: String },

    #[error("no viable solver for intent {intent_id}")]
    NoViableSolver { intent_id: String },

    #[error("insufficient fill for intent {intent_id}: filled {filled}, minimum {minimum}")]
    InsufficientFill {
        intent_id: String,
        filled: Uint128,
        minimum: Uint128,
    },

    #[error("settlement failed: {reason}")]
    Settlement { reason: String },

    #[error("settlement timeout for intent {intent_id}")]
    SettlementTimeout { intent_id: String },

    #[error("invalid configuration: {reason}")]
    InvalidConfiguration { reason: String },

    #[error("IBC transfer failed: {reason}")]
    IbcTransferFailed { reason: String },

    #[error("escrow lock failed: {reason}")]
    EscrowLockFailed { reason: String },

    #[error("vault lock failed: {reason}")]
    VaultLockFailed { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{
        Asset, ExecutionConstraints, ExecutionPlan, FillConfig, FillStrategy, OutputSpec,
        ProposedFill, Settlement, SettlementStatus,
    };
    use cosmwasm_std::Binary;
    use rust_decimal::Decimal;
    use std::collections::HashSet;
    use std::str::FromStr;

    // ═══════════════════════════════════════════════════════════════════
    // MOCK SETTLEMENT MANAGER
    // ═══════════════════════════════════════════════════════════════════

    struct MockSettlementMgr {
        status: SettlementStatus,
    }

    impl MockSettlementMgr {
        fn completing() -> Self {
            Self {
                status: SettlementStatus::Complete,
            }
        }

        fn timing_out() -> Self {
            Self {
                status: SettlementStatus::TimedOut,
            }
        }
    }

    #[async_trait::async_trait]
    impl SettlementManager for MockSettlementMgr {
        async fn execute_settlement(
            &self,
            intent: &Intent,
            solution: &Solution,
            _current_time: u64,
        ) -> Result<Settlement, SettlementError> {
            Ok(Settlement {
                intent_id: intent.id.clone(),
                solver_id: solution.solver_id.clone(),
                user_input: intent.input.amount,
                solver_output: solution.fill.output_amount,
                ibc_transfers: vec![],
                status: self.status.clone(),
            })
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // HELPERS
    // ═══════════════════════════════════════════════════════════════════

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

    fn make_solution(solver_id: &str, intent_id: &str, input: u128, output: u128) -> Solution {
        Solution {
            solver_id: solver_id.to_string(),
            intent_id: intent_id.to_string(),
            fill: ProposedFill {
                input_amount: Uint128::new(input),
                output_amount: Uint128::new(output),
                price: "10.5".to_string(),
            },
            execution: ExecutionPlan::DexRoute { steps: vec![] },
            valid_until: 9999999999,
            bond: Uint128::new(input * 15 / 10),
        }
    }

    fn make_coordinator(settlement_mgr: MockSettlementMgr) -> ExecutionCoordinator {
        let mut pairs = HashSet::new();
        pairs.insert(atom_intents_types::TradingPair::new("uatom", "uusdc"));
        pairs.insert(atom_intents_types::TradingPair::new("uosmo", "uusdc"));
        let validator = Arc::new(IntentValidator::new(
            pairs,
            86400,
            Uint128::new(1000),
        ));

        let matching_engine = Arc::new(tokio::sync::Mutex::new(
            atom_intents_matching_engine::MatchingEngine::new(),
        ));

        let oracle = Arc::new(atom_intents_solver::MockOracle::new("test-oracle"));
        let aggregator = Arc::new(atom_intents_solver::SolutionAggregator::with_price_requirement(
            vec![],
            oracle,
            atom_intents_solver::OraclePriceRequirement::Optional(
                Decimal::from_str("10.5").unwrap(),
            ),
        ));

        ExecutionCoordinator::new(
            validator,
            matching_engine,
            aggregator,
            Arc::new(settlement_mgr),
            TimeoutConfig::default(),
        )
    }

    // ═══════════════════════════════════════════════════════════════════
    // BASIC TYPE TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_execution_stage_equality() {
        assert_eq!(ExecutionStage::Validating, ExecutionStage::Validating);
        assert_ne!(ExecutionStage::Validating, ExecutionStage::Matching);
    }

    #[test]
    fn test_solver_fill_info_creation() {
        let fill_info = SolverFillInfo {
            solver_id: "solver-1".to_string(),
            input_amount: Uint128::new(1_000_000),
            output_amount: Uint128::new(10_000_000),
        };

        assert_eq!(fill_info.solver_id, "solver-1");
        assert_eq!(fill_info.input_amount, Uint128::new(1_000_000));
        assert_eq!(fill_info.output_amount, Uint128::new(10_000_000));
    }

    // ═══════════════════════════════════════════════════════════════════
    // SELECT EXECUTION PATH TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_fully_matched_path() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let intent = make_test_intent("i1", 1_000_000, 10_000_000, true);

        // Fully matched: no solver solutions, total_input >= input.amount
        let plan = OptimalFillPlan {
            selected: vec![],
            total_input: Uint128::new(1_000_000),
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_ok());
        match result.unwrap() {
            ExecutionPath::FullyMatched { amount } => {
                assert_eq!(amount, Uint128::new(1_000_000));
            }
            _ => panic!("Expected FullyMatched"),
        }
    }

    #[test]
    fn test_no_viable_solver() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let intent = make_test_intent("i1", 1_000_000, 10_000_000, true);

        // No solutions, total_input < input.amount
        let plan = OptimalFillPlan {
            selected: vec![],
            total_input: Uint128::new(500_000),
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::NoViableSolver { .. }
        ));
    }

    #[test]
    fn test_requires_solver_path() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        let solution = make_solution("solver-1", "i1", 1_000_000, 10_500_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(1_000_000))],
            total_input: Uint128::new(1_000_000),
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_ok());
        match result.unwrap() {
            ExecutionPath::RequiresSolver {
                matched_amount,
                solver_solutions,
            } => {
                assert_eq!(matched_amount, Uint128::new(1_000_000));
                assert_eq!(solver_solutions.len(), 1);
                assert_eq!(solver_solutions[0].solver_id, "solver-1");
            }
            _ => panic!("Expected RequiresSolver"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // FILL STRATEGY TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_all_or_nothing_insufficient() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.strategy = FillStrategy::AllOrNothing;
        let solution = make_solution("s1", "i1", 500_000, 5_000_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(500_000))],
            total_input: Uint128::new(500_000), // Only half filled
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::InsufficientFill { .. }
        ));
    }

    #[test]
    fn test_all_or_nothing_sufficient() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.strategy = FillStrategy::AllOrNothing;
        let solution = make_solution("s1", "i1", 1_000_000, 10_500_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(1_000_000))],
            total_input: Uint128::new(1_000_000), // Fully filled
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_ok());
        match result.unwrap() {
            ExecutionPath::RequiresSolver { matched_amount, .. } => {
                assert_eq!(matched_amount, Uint128::new(1_000_000));
            }
            _ => panic!("Expected RequiresSolver"),
        }
    }

    #[test]
    fn test_partial_not_allowed_insufficient() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        // allow_partial=false but strategy is Eager (requires_full_fill = true)
        let intent = make_test_intent("i1", 1_000_000, 10_000_000, false);
        let solution = make_solution("s1", "i1", 500_000, 5_000_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(500_000))],
            total_input: Uint128::new(500_000),
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::InsufficientFill { .. }
        ));
    }

    #[test]
    fn test_eager_partial_below_min_amount() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        // min_fill_amount is 500_000 (input/2), send less
        intent.fill_config.min_fill_amount = Uint128::new(500_000);
        let solution = make_solution("s1", "i1", 100_000, 1_000_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(100_000))],
            total_input: Uint128::new(100_000), // Below min_fill_amount
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::InsufficientFill { .. }
        ));
    }

    #[test]
    fn test_eager_partial_below_min_pct() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.min_fill_amount = Uint128::zero(); // No min amount
        intent.fill_config.min_fill_pct = "0.5".to_string(); // 50% minimum
        let solution = make_solution("s1", "i1", 400_000, 4_000_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(400_000))],
            total_input: Uint128::new(400_000), // 40% < 50%
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::InsufficientFill { .. }
        ));
    }

    #[test]
    fn test_eager_partial_above_min_pct() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.min_fill_amount = Uint128::zero();
        intent.fill_config.min_fill_pct = "0.5".to_string();
        let solution = make_solution("s1", "i1", 600_000, 6_000_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(600_000))],
            total_input: Uint128::new(600_000), // 60% > 50%
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_ok());
    }

    #[test]
    fn test_minimum_then_eager_below_strategy_pct() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.min_fill_amount = Uint128::zero();
        intent.fill_config.min_fill_pct = "0.3".to_string(); // Base 30%
        intent.fill_config.strategy = FillStrategy::MinimumThenEager {
            min_pct: "0.8".to_string(), // Strategy requires 80%
        };
        let solution = make_solution("s1", "i1", 500_000, 5_000_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(500_000))],
            total_input: Uint128::new(500_000), // 50% < max(80%, 30%) = 80%
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::InsufficientFill { .. }
        ));
    }

    #[test]
    fn test_minimum_then_eager_above_strategy_pct() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.min_fill_amount = Uint128::zero();
        intent.fill_config.min_fill_pct = "0.3".to_string();
        intent.fill_config.strategy = FillStrategy::MinimumThenEager {
            min_pct: "0.8".to_string(),
        };
        let solution = make_solution("s1", "i1", 900_000, 9_000_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(900_000))],
            total_input: Uint128::new(900_000), // 90% > 80%
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_ok());
    }

    #[test]
    fn test_solver_discretion_accepts_any() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.min_fill_amount = Uint128::zero();
        intent.fill_config.min_fill_pct = "0.1".to_string(); // 10% minimum
        intent.fill_config.strategy = FillStrategy::SolverDiscretion;
        let solution = make_solution("s1", "i1", 200_000, 2_000_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(200_000))],
            total_input: Uint128::new(200_000), // 20% > 10%
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_solver_solutions() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        let solution1 = make_solution("s1", "i1", 600_000, 6_300_000);
        let solution2 = make_solution("s2", "i1", 400_000, 4_200_000);

        let plan = OptimalFillPlan {
            selected: vec![
                (solution1, Uint128::new(600_000)),
                (solution2, Uint128::new(400_000)),
            ],
            total_input: Uint128::new(1_000_000),
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_ok());
        match result.unwrap() {
            ExecutionPath::RequiresSolver {
                solver_solutions, ..
            } => {
                assert_eq!(solver_solutions.len(), 2);
            }
            _ => panic!("Expected RequiresSolver"),
        }
    }

    #[test]
    fn test_invalid_min_fill_pct_returns_error() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.min_fill_pct = "not_a_number".to_string();
        let solution = make_solution("s1", "i1", 1_000_000, 10_500_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(1_000_000))],
            total_input: Uint128::new(1_000_000),
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::InvalidConfiguration { .. }
        ));
    }

    #[test]
    fn test_invalid_strategy_min_pct_returns_error() {
        let coord = make_coordinator(MockSettlementMgr::completing());
        let mut intent = make_test_intent("i1", 1_000_000, 10_000_000, true);
        intent.fill_config.min_fill_amount = Uint128::zero();
        intent.fill_config.min_fill_pct = "0.3".to_string();
        intent.fill_config.strategy = FillStrategy::MinimumThenEager {
            min_pct: "bad_value".to_string(),
        };
        let solution = make_solution("s1", "i1", 1_000_000, 10_500_000);

        let plan = OptimalFillPlan {
            selected: vec![(solution, Uint128::new(1_000_000))],
            total_input: Uint128::new(1_000_000),
        };

        let result = coord.select_execution_path(&intent, &plan);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::InvalidConfiguration { .. }
        ));
    }
}
