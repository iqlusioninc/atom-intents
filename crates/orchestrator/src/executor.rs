use async_trait::async_trait;
use atom_intents_matching_engine::MatchingEngine;
use atom_intents_settlement::{
    EscrowContract, EscrowLock, IbcResult, RelayerService, SettlementError, SolverVaultContract,
    TimeoutConfig, TwoPhaseSettlement, VaultLock,
};
use atom_intents_solver::SolutionAggregator;
use atom_intents_types::{Intent, OptimalFillPlan, Solution};
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};

use crate::recovery::{SettlementPhase, SettlementState};
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
        let matched_amount: Uint128 = match_result
            .fills
            .iter()
            .map(|f| f.input_amount)
            .sum();

        info!(
            intent_id = %intent.id,
            matched_amount = %matched_amount,
            "Matched amount from internal orders"
        );

        // 3. Get solver quotes for remaining amount
        info!(intent_id = %intent.id, stage = ?ExecutionStage::SolvingForQuotes, "Getting solver quotes");
        let fill_plan = self
            .solution_aggregator
            .aggregate(&intent, matched_amount)
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
                let best_solution = solver_solutions.first().ok_or_else(|| {
                    ExecutionError::NoViableSolver {
                        intent_id: intent.id.clone(),
                    }
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
                                reason: format!("Unexpected status: {:?}", settlement_result.status),
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
        if intent.fill_config.allow_partial {
            // Check minimum fill amount
            if total_filled < intent.fill_config.min_fill_amount {
                return Err(ExecutionError::InsufficientFill {
                    intent_id: intent.id.clone(),
                    filled: total_filled,
                    minimum: intent.fill_config.min_fill_amount,
                });
            }

            // Check minimum fill percentage
            let fill_pct: f64 = intent
                .fill_config
                .min_fill_pct
                .parse()
                .map_err(|_| ExecutionError::InvalidConfiguration {
                    reason: "Invalid min_fill_pct".to_string(),
                })?;

            let actual_pct =
                total_filled.u128() as f64 / intent.input.amount.u128() as f64;

            if actual_pct < fill_pct {
                return Err(ExecutionError::InsufficientFill {
                    intent_id: intent.id.clone(),
                    filled: total_filled,
                    minimum: Uint128::new(
                        (intent.input.amount.u128() as f64 * fill_pct) as u128,
                    ),
                });
            }
        } else {
            // Full fill required
            if total_filled < intent.input.amount {
                return Err(ExecutionError::InsufficientFill {
                    intent_id: intent.id.clone(),
                    filled: total_filled,
                    minimum: intent.input.amount,
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
        Asset, ExecutionConstraints, FillConfig, FillStrategy, OutputSpec,
    };
    use cosmwasm_std::Binary;

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
}
