use atom_intents_matching_engine::MatchingEngine;
use atom_intents_relayer::SolverRelayer;
use atom_intents_settlement::{
    EscrowContract, RelayerService, SolverVaultContract, TimeoutConfig, TwoPhaseSettlement,
};
use atom_intents_solver::SolutionAggregator;
use atom_intents_types::{
    Asset, ExecutionConstraints, FillConfig, Intent, OutputSpec, SolverQuote,
    TradingPair,
};
use cosmwasm_std::Uint128;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info, warn};

use crate::executor::{ExecutionCoordinator, ExecutionError, ExecutionOutcome, SettlementManager};
use crate::recovery::{RecoveryAction, RecoveryResult, SettlementState};
use crate::validator::IntentValidator;

/// Adapter to wrap TwoPhaseSettlement as SettlementManager trait object
struct TwoPhaseSettlementAdapter<E, V, R>
where
    E: EscrowContract,
    V: SolverVaultContract,
    R: RelayerService,
{
    settlement: TwoPhaseSettlement<E, V, R>,
}

impl<E, V, R> TwoPhaseSettlementAdapter<E, V, R>
where
    E: EscrowContract,
    V: SolverVaultContract,
    R: RelayerService,
{
    fn new(user_escrow: E, solver_vault: V, relayer: R, config: TimeoutConfig) -> Self {
        Self {
            settlement: TwoPhaseSettlement::new(user_escrow, solver_vault, relayer, config),
        }
    }
}

#[async_trait::async_trait]
impl<E, V, R> SettlementManager for TwoPhaseSettlementAdapter<E, V, R>
where
    E: EscrowContract + 'static,
    V: SolverVaultContract + 'static,
    R: RelayerService + 'static,
{
    async fn execute_settlement(
        &self,
        intent: &Intent,
        solution: &atom_intents_types::Solution,
        current_time: u64,
    ) -> Result<atom_intents_types::Settlement, atom_intents_settlement::SettlementError> {
        self.settlement
            .execute(intent, solution, current_time)
            .await
    }
}

/// Configuration for the orchestrator
#[derive(Clone, Debug)]
pub struct OrchestratorConfig {
    /// Batch auction interval (milliseconds)
    pub batch_interval_ms: u64,

    /// Maximum intents per batch
    pub max_batch_size: usize,

    /// Settlement timeout threshold
    pub settlement_timeout_threshold: u64,

    /// Enable automatic recovery
    pub auto_recovery_enabled: bool,
}

impl OrchestratorConfig {
    pub fn with_recovery(mut self, enabled: bool) -> Self {
        self.auto_recovery_enabled = enabled;
        self
    }
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            batch_interval_ms: 5000, // 5 seconds
            max_batch_size: 100,
            settlement_timeout_threshold: 600, // 10 minutes
            auto_recovery_enabled: true,
        }
    }
}

/// Builder error
#[derive(Debug, Error)]
pub enum BuilderError {
    #[error("missing required field: {field}")]
    MissingField { field: String },
}

/// Builder for IntentOrchestrator
pub struct IntentOrchestratorBuilder<E, V, R>
where
    E: EscrowContract + 'static,
    V: SolverVaultContract + 'static,
    R: RelayerService + 'static,
{
    validator: Option<Arc<IntentValidator>>,
    matching_engine: Option<MatchingEngine>,
    solution_aggregator: Option<Arc<SolutionAggregator>>,
    user_escrow: Option<E>,
    solver_vault: Option<V>,
    relayer: Option<Arc<SolverRelayer>>,
    relayer_service: Option<R>,
    config: OrchestratorConfig,
    timeout_config: TimeoutConfig,
}

impl<E, V, R> IntentOrchestratorBuilder<E, V, R>
where
    E: EscrowContract + 'static,
    V: SolverVaultContract + 'static,
    R: RelayerService + 'static,
{
    /// Create a new builder with defaults
    pub fn new() -> Self {
        Self {
            validator: None,
            matching_engine: None,
            solution_aggregator: None,
            user_escrow: None,
            solver_vault: None,
            relayer: None,
            relayer_service: None,
            config: OrchestratorConfig::default(),
            timeout_config: TimeoutConfig::default(),
        }
    }

    /// Set the intent validator
    pub fn with_validator(mut self, validator: Arc<IntentValidator>) -> Self {
        self.validator = Some(validator);
        self
    }

    /// Set the matching engine
    pub fn with_matching_engine(mut self, matching_engine: MatchingEngine) -> Self {
        self.matching_engine = Some(matching_engine);
        self
    }

    /// Set the solution aggregator
    pub fn with_solution_aggregator(mut self, solution_aggregator: Arc<SolutionAggregator>) -> Self {
        self.solution_aggregator = Some(solution_aggregator);
        self
    }

    /// Set the user escrow contract
    pub fn with_user_escrow(mut self, user_escrow: E) -> Self {
        self.user_escrow = Some(user_escrow);
        self
    }

    /// Set the solver vault contract
    pub fn with_solver_vault(mut self, solver_vault: V) -> Self {
        self.solver_vault = Some(solver_vault);
        self
    }

    /// Set the solver relayer
    pub fn with_relayer(mut self, relayer: Arc<SolverRelayer>) -> Self {
        self.relayer = Some(relayer);
        self
    }

    /// Set the relayer service
    pub fn with_relayer_service(mut self, relayer_service: R) -> Self {
        self.relayer_service = Some(relayer_service);
        self
    }

    /// Set the orchestrator configuration
    pub fn with_config(mut self, config: OrchestratorConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the timeout configuration
    pub fn with_timeout_config(mut self, timeout_config: TimeoutConfig) -> Self {
        self.timeout_config = timeout_config;
        self
    }

    /// Build the IntentOrchestrator, validating that all required fields are set
    pub fn build(self) -> Result<IntentOrchestrator, BuilderError> {
        let validator = self.validator.ok_or_else(|| BuilderError::MissingField {
            field: "validator".to_string(),
        })?;

        let matching_engine = self.matching_engine.ok_or_else(|| BuilderError::MissingField {
            field: "matching_engine".to_string(),
        })?;

        let solution_aggregator = self.solution_aggregator.ok_or_else(|| BuilderError::MissingField {
            field: "solution_aggregator".to_string(),
        })?;

        let user_escrow = self.user_escrow.ok_or_else(|| BuilderError::MissingField {
            field: "user_escrow".to_string(),
        })?;

        let solver_vault = self.solver_vault.ok_or_else(|| BuilderError::MissingField {
            field: "solver_vault".to_string(),
        })?;

        let relayer = self.relayer.ok_or_else(|| BuilderError::MissingField {
            field: "relayer".to_string(),
        })?;

        let relayer_service = self.relayer_service.ok_or_else(|| BuilderError::MissingField {
            field: "relayer_service".to_string(),
        })?;

        // Use the new() method to construct the orchestrator
        Ok(IntentOrchestrator::new(
            validator,
            matching_engine,
            solution_aggregator,
            user_escrow,
            solver_vault,
            relayer,
            relayer_service,
            self.config,
            self.timeout_config,
        ))
    }
}

impl<E, V, R> Default for IntentOrchestratorBuilder<E, V, R>
where
    E: EscrowContract + 'static,
    V: SolverVaultContract + 'static,
    R: RelayerService + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Main orchestrator coordinating the full intent execution flow
pub struct IntentOrchestrator {
    matching_engine: Arc<Mutex<MatchingEngine>>,
    solution_aggregator: Arc<SolutionAggregator>,
    relayer: Arc<SolverRelayer>,
    executor: Arc<ExecutionCoordinator>,
    config: OrchestratorConfig,
    /// Track intent status
    intent_statuses: Arc<RwLock<HashMap<String, IntentStatus>>>,
    /// Track active settlements
    active_settlements: Arc<RwLock<HashMap<String, SettlementState>>>,
}

impl IntentOrchestrator {
    /// Create a new builder for constructing an IntentOrchestrator
    pub fn builder<E, V, R>() -> IntentOrchestratorBuilder<E, V, R>
    where
        E: EscrowContract + 'static,
        V: SolverVaultContract + 'static,
        R: RelayerService + 'static,
    {
        IntentOrchestratorBuilder::new()
    }

    /// Create a new IntentOrchestrator (constructor for backwards compatibility)
    pub fn new<E, V, R>(
        validator: Arc<IntentValidator>,
        matching_engine: MatchingEngine,
        solution_aggregator: Arc<SolutionAggregator>,
        user_escrow: E,
        solver_vault: V,
        relayer: Arc<SolverRelayer>,
        relayer_service: R,
        config: OrchestratorConfig,
        timeout_config: TimeoutConfig,
    ) -> Self
    where
        E: EscrowContract + 'static,
        V: SolverVaultContract + 'static,
        R: RelayerService + 'static,
    {
        let matching_engine = Arc::new(Mutex::new(matching_engine));

        // Create settlement manager and wrap in trait object
        let settlement_manager: Arc<dyn crate::executor::SettlementManager> =
            Arc::new(TwoPhaseSettlementAdapter::new(
                user_escrow,
                solver_vault,
                relayer_service,
                timeout_config.clone(),
            ));

        let executor = Arc::new(ExecutionCoordinator::new(
            validator,
            matching_engine.clone(),
            solution_aggregator.clone(),
            settlement_manager,
            timeout_config,
        ));

        Self {
            matching_engine,
            solution_aggregator,
            relayer,
            executor,
            config,
            intent_statuses: Arc::new(RwLock::new(HashMap::new())),
            active_settlements: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Process a single intent end-to-end
    pub async fn process_intent(
        &self,
        intent: Intent,
    ) -> Result<ExecutionResult, OrchestratorError> {
        let intent_id = intent.id.clone();
        info!(intent_id = %intent_id, "Processing intent");

        // Update status to matching
        self.update_status(&intent_id, IntentStatus::Matching).await;

        // Get current timestamp
        let current_time = current_timestamp();

        // Execute the intent
        match self
            .executor
            .coordinate_execution(intent.clone(), current_time)
            .await
        {
            Ok(ExecutionOutcome::Completed {
                matched_amount,
                solver_fills,
                settlement_id,
                ..
            }) => {
                // Calculate total output and execution price
                let total_output: Uint128 = solver_fills.iter().map(|f| f.output_amount).sum();

                let execution_price = if !matched_amount.is_zero() {
                    Decimal::from(total_output.u128()) / Decimal::from(matched_amount.u128())
                } else {
                    Decimal::ZERO
                };

                let result = ExecutionResult {
                    intent_id: intent_id.clone(),
                    input_amount: matched_amount,
                    output_amount: total_output,
                    execution_price,
                    solver_id: solver_fills.first().map(|f| f.solver_id.clone()),
                    settlement_id: settlement_id.clone().unwrap_or_else(|| intent_id.clone()),
                    completed_at: current_time,
                };

                self.update_status(
                    &intent_id,
                    IntentStatus::Completed {
                        result: result.clone(),
                    },
                )
                .await;

                info!(intent_id = %intent_id, "Intent completed successfully");
                Ok(result)
            }
            Ok(ExecutionOutcome::Failed { stage, error, .. }) => {
                error!(
                    intent_id = %intent_id,
                    stage = ?stage,
                    error = %error,
                    "Intent execution failed"
                );

                self.update_status(
                    &intent_id,
                    IntentStatus::Failed {
                        error: error.to_string(),
                        recovery: None,
                    },
                )
                .await;

                Err(OrchestratorError::Execution { source: error })
            }
            Err(e) => {
                error!(intent_id = %intent_id, error = %e, "Intent execution error");

                self.update_status(
                    &intent_id,
                    IntentStatus::Failed {
                        error: e.to_string(),
                        recovery: None,
                    },
                )
                .await;

                Err(OrchestratorError::Execution { source: e })
            }
        }
    }

    /// Process a batch of intents (batch auction)
    pub async fn process_batch(
        &self,
        intents: Vec<Intent>,
    ) -> Result<BatchResult, OrchestratorError> {
        if intents.is_empty() {
            return Ok(BatchResult {
                results: Vec::new(),
                internal_crosses: 0,
                solver_fills: 0,
                failed: Vec::new(),
            });
        }

        info!(batch_size = intents.len(), "Processing batch auction");

        // Group intents by trading pair
        let mut pairs: HashMap<TradingPair, Vec<Intent>> = HashMap::new();
        for intent in intents {
            let pair = intent.pair();
            pairs.entry(pair).or_insert_with(Vec::new).push(intent);
        }

        let mut all_results = Vec::new();
        let all_failed = Vec::new();
        let mut total_internal_crosses = 0;
        let mut total_solver_fills = 0;

        // Process each pair's batch auction
        for (pair, pair_intents) in pairs {
            info!(
                pair = ?pair,
                count = pair_intents.len(),
                "Running batch auction for pair"
            );

            // Get solver quotes for this pair
            let solver_quotes = self.get_solver_quotes(&pair).await;

            // SECURITY FIX (1.1): Get oracle price with confidence
            let (oracle_price, oracle_confidence) = self
                .solution_aggregator
                .get_oracle_price_with_confidence(&pair)
                .await
                .ok()
                .unwrap_or((Decimal::TEN, Decimal::from_str("0.01").unwrap()));

            // Get current time for expiration checks
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            // Run batch auction with full security validation (1.1, 1.4, 1.5, 1.6)
            let auction_result = {
                let mut engine = self.matching_engine.lock().await;
                engine
                    .run_batch_auction_with_confidence(
                        pair.clone(),
                        pair_intents.clone(),
                        solver_quotes,
                        oracle_price,
                        Some(oracle_confidence),
                        current_time,
                    )
                    .map_err(|e| OrchestratorError::BatchAuction {
                        reason: e.to_string(),
                    })?
            };

            // Process auction results
            total_internal_crosses += auction_result.internal_fills.len() as u32;
            total_solver_fills += auction_result.solver_fills.len() as u32;

            // Convert fills to execution results
            for fill in auction_result.internal_fills {
                let result = ExecutionResult {
                    intent_id: fill.intent_id.clone(),
                    input_amount: fill.input_amount,
                    output_amount: fill.output_amount,
                    execution_price: if !fill.input_amount.is_zero() {
                        Decimal::from(fill.output_amount.u128())
                            / Decimal::from(fill.input_amount.u128())
                    } else {
                        Decimal::ZERO
                    },
                    solver_id: None,
                    settlement_id: format!("auction-{}", auction_result.epoch_id),
                    completed_at: current_timestamp(),
                };
                all_results.push(result);
            }

            for fill in auction_result.solver_fills {
                let result = ExecutionResult {
                    intent_id: fill.intent_id.clone(),
                    input_amount: fill.input_amount,
                    output_amount: fill.output_amount,
                    execution_price: if !fill.input_amount.is_zero() {
                        Decimal::from(fill.output_amount.u128())
                            / Decimal::from(fill.input_amount.u128())
                    } else {
                        Decimal::ZERO
                    },
                    solver_id: Some(fill.counterparty.clone()),
                    settlement_id: format!("auction-{}", auction_result.epoch_id),
                    completed_at: current_timestamp(),
                };
                all_results.push(result);
            }
        }

        Ok(BatchResult {
            results: all_results,
            internal_crosses: total_internal_crosses,
            solver_fills: total_solver_fills,
            failed: all_failed,
        })
    }

    /// Get status of an intent/settlement
    pub async fn get_status(&self, intent_id: &str) -> Result<IntentStatus, OrchestratorError> {
        let statuses = self.intent_statuses.read().await;
        statuses
            .get(intent_id)
            .cloned()
            .ok_or_else(|| OrchestratorError::IntentNotFound {
                intent_id: intent_id.to_string(),
            })
    }

    /// Cancel a pending intent
    pub async fn cancel_intent(&self, intent_id: &str) -> Result<(), OrchestratorError> {
        info!(intent_id = %intent_id, "Cancelling intent");

        // Check current status
        let status = self.get_status(intent_id).await?;

        match status {
            IntentStatus::Pending | IntentStatus::Matching => {
                // Can cancel
                self.update_status(intent_id, IntentStatus::Cancelled).await;
                Ok(())
            }
            IntentStatus::Executing { .. } => Err(OrchestratorError::CannotCancel {
                intent_id: intent_id.to_string(),
                reason: "Intent is currently executing".to_string(),
            }),
            IntentStatus::Completed { .. } => Err(OrchestratorError::CannotCancel {
                intent_id: intent_id.to_string(),
                reason: "Intent already completed".to_string(),
            }),
            IntentStatus::Failed { .. } | IntentStatus::Cancelled => {
                // Already in terminal state
                Ok(())
            }
        }
    }

    /// Run recovery process for stuck settlements
    pub async fn run_recovery(&self) -> Result<Vec<RecoveryResult>, OrchestratorError> {
        if !self.config.auto_recovery_enabled {
            return Ok(Vec::new());
        }

        info!("Running recovery process");

        let _settlements: Vec<SettlementState> = {
            let active = self.active_settlements.read().await;
            active.values().cloned().collect()
        };

        // Recovery not implemented in this version - would require holding recovery_manager
        // For now, just return empty results
        Ok(Vec::new())
    }

    /// Update intent status
    pub async fn update_status(&self, intent_id: &str, status: IntentStatus) {
        let mut statuses = self.intent_statuses.write().await;
        statuses.insert(intent_id.to_string(), status);
    }

    /// Get solver quotes for a trading pair
    async fn get_solver_quotes(&self, pair: &TradingPair) -> Vec<SolverQuote> {
        // Get oracle price for the pair
        let oracle_price = match self.solution_aggregator.get_oracle_price(pair).await {
            Ok(price) => price,
            Err(e) => {
                warn!(
                    pair = ?pair,
                    error = %e,
                    "Failed to get oracle price for solver quotes, using default"
                );
                Decimal::TEN
            }
        };

        // Create a representative intent amount for getting quotes (1000 units)
        let quote_amount = Uint128::new(1_000_000_000); // 1000 in micro units

        // Create a sample intent for the pair to get quotes
        // Use base as input and quote as output (standard market convention)
        let sample_intent = Intent {
            id: "quote-sample".to_string(),
            version: "1.0".to_string(),
            nonce: 0,
            user: "quote-requester".to_string(),
            input: Asset {
                chain_id: "cosmoshub-4".to_string(),
                denom: pair.base.clone(),
                amount: quote_amount,
            },
            output: OutputSpec {
                chain_id: "cosmoshub-4".to_string(),
                denom: pair.quote.clone(),
                min_amount: Uint128::zero(),
                limit_price: "0".to_string(),
                recipient: "quote-requester".to_string(),
            },
            fill_config: FillConfig::default(),
            constraints: ExecutionConstraints {
                deadline: current_timestamp() + 3600,
                max_hops: Some(3),
                excluded_venues: vec![],
                max_solver_fee_bps: Some(50),
                allow_cross_ecosystem: false,
                max_bridge_time_secs: None,
            },
            signature: cosmwasm_std::Binary::default(),
            public_key: cosmwasm_std::Binary::default(),
            created_at: current_timestamp(),
            expires_at: current_timestamp() + 3600,
        };

        let ctx = atom_intents_types::SolveContext {
            matched_amount: Uint128::zero(),
            remaining: quote_amount,
            oracle_price: oracle_price.to_string(),
        };

        // Query all solvers that support this pair
        let solvers = self.solution_aggregator.solvers();
        let mut quotes = Vec::new();

        for solver in solvers.iter() {
            if !solver.supported_pairs().contains(pair) {
                continue;
            }

            match solver.solve(&sample_intent, &ctx).await {
                Ok(solution) => {
                    // Convert Solution to SolverQuote
                    let quote = SolverQuote {
                        solver_id: solution.solver_id,
                        input_amount: solution.fill.input_amount,
                        output_amount: solution.fill.output_amount,
                        price: solution.fill.price,
                        valid_for_ms: (solution.valid_until.saturating_sub(current_timestamp()))
                            * 1000,
                    };
                    quotes.push(quote);
                }
                Err(e) => {
                    warn!(
                        solver_id = solver.id(),
                        pair = ?pair,
                        error = %e,
                        "Solver failed to provide quote"
                    );
                }
            }
        }

        info!(
            pair = ?pair,
            quote_count = quotes.len(),
            "Retrieved solver quotes"
        );

        quotes
    }
}

/// Intent status in the system
#[derive(Debug, Clone)]
pub enum IntentStatus {
    /// Submitted but not yet processed
    Pending,

    /// In matching engine
    Matching,

    /// Executing
    Executing {
        stage: crate::executor::ExecutionStage,
    },

    /// Completed successfully
    Completed { result: ExecutionResult },

    /// Failed with error
    Failed {
        error: String,
        recovery: Option<RecoveryAction>,
    },

    /// Cancelled by user
    Cancelled,
}

/// Result of intent execution
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub intent_id: String,
    pub input_amount: Uint128,
    pub output_amount: Uint128,
    pub execution_price: Decimal,
    pub solver_id: Option<String>,
    pub settlement_id: String,
    pub completed_at: u64,
}

/// Result of batch auction
#[derive(Debug)]
pub struct BatchResult {
    pub results: Vec<ExecutionResult>,
    pub internal_crosses: u32,
    pub solver_fills: u32,
    pub failed: Vec<(String, OrchestratorError)>,
}

/// Orchestrator errors
#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("execution failed: {source}")]
    Execution {
        #[from]
        source: ExecutionError,
    },

    #[error("batch auction failed: {reason}")]
    BatchAuction { reason: String },

    #[error("intent not found: {intent_id}")]
    IntentNotFound { intent_id: String },

    #[error("cannot cancel intent {intent_id}: {reason}")]
    CannotCancel { intent_id: String, reason: String },

    #[error("settlement not found: {settlement_id}")]
    SettlementNotFound { settlement_id: String },
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time is before UNIX epoch - clock error")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_orchestrator_config_default() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.batch_interval_ms, 5000);
        assert_eq!(config.max_batch_size, 100);
        assert!(config.auto_recovery_enabled);
    }

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

    #[test]
    fn test_builder_pattern() {
        use atom_intents_matching_engine::MatchingEngine;
        use atom_intents_settlement::{
            EscrowContract, EscrowLock, IbcResult, RelayerService, SettlementError,
            SolverVaultContract, TimeoutConfig, VaultLock,
        };
        use atom_intents_solver::{MockOracle, SolutionAggregator};
        use std::sync::Arc;

        // Simple mock implementations for testing
        #[derive(Clone)]
        struct TestEscrow;
        #[async_trait::async_trait]
        impl EscrowContract for TestEscrow {
            async fn lock(
                &self,
                _user: &str,
                _amount: Uint128,
                _denom: &str,
                _timeout: u64,
            ) -> Result<EscrowLock, SettlementError> {
                unimplemented!()
            }
            async fn release_to(
                &self,
                _lock: &EscrowLock,
                _recipient: &str,
            ) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn refund(&self, _lock: &EscrowLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestVault;
        #[async_trait::async_trait]
        impl SolverVaultContract for TestVault {
            async fn lock(
                &self,
                _solver_id: &str,
                _amount: Uint128,
                _denom: &str,
                _timeout: u64,
            ) -> Result<VaultLock, SettlementError> {
                unimplemented!()
            }
            async fn unlock(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn mark_complete(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestRelayerService;
        #[async_trait::async_trait]
        impl RelayerService for TestRelayerService {
            async fn track_settlement(
                &self,
                _settlement_id: &str,
                _transfers: &[atom_intents_types::IbcTransferInfo],
            ) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn wait_for_ibc(
                &self,
                _transfer: &atom_intents_types::IbcTransferInfo,
            ) -> Result<IbcResult, SettlementError> {
                unimplemented!()
            }
        }

        // Test builder pattern
        let validator = Arc::new(IntentValidator::default_config());
        let matching_engine = MatchingEngine::new();

        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let solvers = vec![];
        let aggregator = Arc::new(SolutionAggregator::new(solvers, oracle));

        let escrow = TestEscrow;
        let vault = TestVault;
        let relayer_service = TestRelayerService;

        use atom_intents_relayer::RelayerConfig;
        let relayer_config = RelayerConfig {
            solver_id: "test-solver".to_string(),
            chains: vec![],
            poll_interval_ms: 1000,
            batch_size: 10,
        };
        let chain_clients = std::collections::HashMap::new();
        let relayer = Arc::new(atom_intents_relayer::SolverRelayer::new(
            relayer_config,
            chain_clients,
        ));

        // Use builder pattern
        let result = IntentOrchestrator::builder()
            .with_validator(validator)
            .with_matching_engine(matching_engine)
            .with_solution_aggregator(aggregator)
            .with_user_escrow(escrow)
            .with_solver_vault(vault)
            .with_relayer(relayer)
            .with_relayer_service(relayer_service)
            .with_config(OrchestratorConfig::default())
            .with_timeout_config(TimeoutConfig::default())
            .build();

        assert!(result.is_ok(), "Builder should succeed with all fields set");
    }

    #[test]
    fn test_builder_missing_fields() {
        use atom_intents_settlement::{
            EscrowContract, EscrowLock, IbcResult, RelayerService, SettlementError,
            SolverVaultContract, VaultLock,
        };

        // Simple mock implementations for testing
        #[derive(Clone)]
        struct TestEscrow;
        #[async_trait::async_trait]
        impl EscrowContract for TestEscrow {
            async fn lock(
                &self,
                _user: &str,
                _amount: Uint128,
                _denom: &str,
                _timeout: u64,
            ) -> Result<EscrowLock, SettlementError> {
                unimplemented!()
            }
            async fn release_to(
                &self,
                _lock: &EscrowLock,
                _recipient: &str,
            ) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn refund(&self, _lock: &EscrowLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestVault;
        #[async_trait::async_trait]
        impl SolverVaultContract for TestVault {
            async fn lock(
                &self,
                _solver_id: &str,
                _amount: Uint128,
                _denom: &str,
                _timeout: u64,
            ) -> Result<VaultLock, SettlementError> {
                unimplemented!()
            }
            async fn unlock(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn mark_complete(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestRelayerService;
        #[async_trait::async_trait]
        impl RelayerService for TestRelayerService {
            async fn track_settlement(
                &self,
                _settlement_id: &str,
                _transfers: &[atom_intents_types::IbcTransferInfo],
            ) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn wait_for_ibc(
                &self,
                _transfer: &atom_intents_types::IbcTransferInfo,
            ) -> Result<IbcResult, SettlementError> {
                unimplemented!()
            }
        }

        // Test builder with missing required fields
        let result: Result<IntentOrchestrator, BuilderError> =
            IntentOrchestrator::builder::<TestEscrow, TestVault, TestRelayerService>().build();

        assert!(
            result.is_err(),
            "Builder should fail when required fields are missing"
        );

        if let Err(BuilderError::MissingField { field }) = result {
            assert_eq!(field, "validator", "Should report missing validator");
        } else {
            panic!("Expected MissingField error");
        }
    }

    #[tokio::test]
    async fn test_get_solver_quotes_returns_quotes() {
        use atom_intents_matching_engine::MatchingEngine;
        use atom_intents_settlement::{
            EscrowContract, EscrowLock, IbcResult, RelayerService, SettlementError,
            SolverVaultContract, TimeoutConfig, VaultLock,
        };
        use atom_intents_solver::{
            DexRoutingSolver, MockDexClient, MockOracle, SolutionAggregator,
        };
        use std::sync::Arc;

        // Simple mock implementations for testing
        #[derive(Clone)]
        struct TestEscrow;
        #[async_trait::async_trait]
        impl EscrowContract for TestEscrow {
            async fn lock(
                &self,
                _user: &str,
                _amount: Uint128,
                _denom: &str,
                _timeout: u64,
            ) -> Result<EscrowLock, SettlementError> {
                unimplemented!()
            }
            async fn release_to(
                &self,
                _lock: &EscrowLock,
                _recipient: &str,
            ) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn refund(&self, _lock: &EscrowLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestVault;
        #[async_trait::async_trait]
        impl SolverVaultContract for TestVault {
            async fn lock(
                &self,
                _solver_id: &str,
                _amount: Uint128,
                _denom: &str,
                _timeout: u64,
            ) -> Result<VaultLock, SettlementError> {
                unimplemented!()
            }
            async fn unlock(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn mark_complete(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestRelayerService;
        #[async_trait::async_trait]
        impl RelayerService for TestRelayerService {
            async fn track_settlement(
                &self,
                _settlement_id: &str,
                _transfers: &[atom_intents_types::IbcTransferInfo],
            ) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn wait_for_ibc(
                &self,
                _transfer: &atom_intents_types::IbcTransferInfo,
            ) -> Result<IbcResult, SettlementError> {
                unimplemented!()
            }
        }

        // Create mock DEX client with good liquidity
        let mock_dex = Arc::new(MockDexClient::new("osmosis", 100_000_000_000, 0.003));

        // Create DEX solver
        let solver = Arc::new(DexRoutingSolver::new("test-solver", vec![mock_dex]));

        // Create mock oracle with price
        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("uatom", "uusdc");
        oracle
            .set_price(
                &pair,
                Decimal::from_str("12.34").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        // Create solution aggregator with solver
        let aggregator = Arc::new(SolutionAggregator::new(vec![solver], oracle));

        // Create orchestrator with minimal setup
        let validator = Arc::new(IntentValidator::default_config());
        let matching_engine = MatchingEngine::new();

        let escrow = TestEscrow;
        let vault = TestVault;
        let relayer_service = TestRelayerService;

        // Create relayer with minimal config
        use atom_intents_relayer::RelayerConfig;
        let relayer_config = RelayerConfig {
            solver_id: "test-solver".to_string(),
            chains: vec![],
            poll_interval_ms: 1000,
            batch_size: 10,
        };
        let chain_clients = std::collections::HashMap::new();
        let relayer = Arc::new(atom_intents_relayer::SolverRelayer::new(
            relayer_config,
            chain_clients,
        ));

        let timeout_config = TimeoutConfig::default();
        let config = OrchestratorConfig::default();

        let orchestrator = IntentOrchestrator::new(
            validator,
            matching_engine,
            aggregator,
            escrow,
            vault,
            relayer,
            relayer_service,
            config,
            timeout_config,
        );

        // Test get_solver_quotes
        let quotes = orchestrator.get_solver_quotes(&pair).await;

        // Verify quotes are returned
        assert!(
            !quotes.is_empty(),
            "get_solver_quotes should return non-empty quotes"
        );
        assert_eq!(
            quotes.len(),
            1,
            "Should have exactly one quote from test-solver"
        );

        // Verify quote contents
        let quote = &quotes[0];
        assert_eq!(quote.solver_id, "test-solver");
        assert!(
            !quote.input_amount.is_zero(),
            "Quote should have non-zero input amount"
        );
        assert!(
            !quote.output_amount.is_zero(),
            "Quote should have non-zero output amount"
        );
        assert!(!quote.price.is_empty(), "Quote should have a price");
        assert!(quote.valid_for_ms > 0, "Quote should have valid_for_ms > 0");
    }

    #[tokio::test]
    async fn test_get_solver_quotes_with_multiple_solvers() {
        use atom_intents_matching_engine::MatchingEngine;
        use atom_intents_settlement::{
            EscrowContract, EscrowLock, IbcResult, RelayerService, SettlementError,
            SolverVaultContract, TimeoutConfig, VaultLock,
        };
        use atom_intents_solver::{
            DexRoutingSolver, MockDexClient, MockOracle, SolutionAggregator,
        };
        use std::sync::Arc;

        // Simple mock implementations for testing
        #[derive(Clone)]
        struct TestEscrow;
        #[async_trait::async_trait]
        impl EscrowContract for TestEscrow {
            async fn lock(
                &self,
                _user: &str,
                _amount: Uint128,
                _denom: &str,
                _timeout: u64,
            ) -> Result<EscrowLock, SettlementError> {
                unimplemented!()
            }
            async fn release_to(
                &self,
                _lock: &EscrowLock,
                _recipient: &str,
            ) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn refund(&self, _lock: &EscrowLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestVault;
        #[async_trait::async_trait]
        impl SolverVaultContract for TestVault {
            async fn lock(
                &self,
                _solver_id: &str,
                _amount: Uint128,
                _denom: &str,
                _timeout: u64,
            ) -> Result<VaultLock, SettlementError> {
                unimplemented!()
            }
            async fn unlock(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn mark_complete(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestRelayerService;
        #[async_trait::async_trait]
        impl RelayerService for TestRelayerService {
            async fn track_settlement(
                &self,
                _settlement_id: &str,
                _transfers: &[atom_intents_types::IbcTransferInfo],
            ) -> Result<(), SettlementError> {
                unimplemented!()
            }
            async fn wait_for_ibc(
                &self,
                _transfer: &atom_intents_types::IbcTransferInfo,
            ) -> Result<IbcResult, SettlementError> {
                unimplemented!()
            }
        }

        // Create two mock DEX clients
        let mock_dex1 = Arc::new(MockDexClient::new("osmosis", 100_000_000_000, 0.003));
        let mock_dex2 = Arc::new(MockDexClient::new("astroport", 80_000_000_000, 0.0025));

        // Create two DEX solvers
        let solver1 = Arc::new(DexRoutingSolver::new("solver-1", vec![mock_dex1]));
        let solver2 = Arc::new(DexRoutingSolver::new("solver-2", vec![mock_dex2]));

        // Create mock oracle
        let oracle = Arc::new(MockOracle::new("test-oracle"));
        let pair = TradingPair::new("uatom", "uusdc");
        oracle
            .set_price(
                &pair,
                Decimal::from_str("12.34").unwrap(),
                Decimal::from_str("0.01").unwrap(),
            )
            .await
            .unwrap();

        // Create solution aggregator with both solvers
        let aggregator = Arc::new(SolutionAggregator::new(vec![solver1, solver2], oracle));

        // Create orchestrator
        let validator = Arc::new(IntentValidator::default_config());
        let matching_engine = MatchingEngine::new();

        let escrow = TestEscrow;
        let vault = TestVault;
        let relayer_service = TestRelayerService;

        // Create relayer with minimal config
        use atom_intents_relayer::RelayerConfig;
        let relayer_config = RelayerConfig {
            solver_id: "test-solver".to_string(),
            chains: vec![],
            poll_interval_ms: 1000,
            batch_size: 10,
        };
        let chain_clients = std::collections::HashMap::new();
        let relayer = Arc::new(atom_intents_relayer::SolverRelayer::new(
            relayer_config,
            chain_clients,
        ));

        let orchestrator = IntentOrchestrator::new(
            validator,
            matching_engine,
            aggregator,
            escrow,
            vault,
            relayer,
            relayer_service,
            OrchestratorConfig::default(),
            TimeoutConfig::default(),
        );

        // Test get_solver_quotes
        let quotes = orchestrator.get_solver_quotes(&pair).await;

        // Verify multiple quotes are returned
        assert!(
            quotes.len() >= 2,
            "Should have quotes from multiple solvers, got {}",
            quotes.len()
        );

        // Verify we have different solver IDs
        let solver_ids: std::collections::HashSet<_> =
            quotes.iter().map(|q| q.solver_id.clone()).collect();
        assert!(
            solver_ids.len() >= 2,
            "Should have quotes from at least 2 different solvers"
        );
    }
}
