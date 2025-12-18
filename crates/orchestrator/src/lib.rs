pub mod executor;
pub mod orchestrator;
pub mod recovery;
pub mod validator;

#[cfg(test)]
mod tests;

// Re-export main types
pub use executor::{
    ExecutionCoordinator, ExecutionError, ExecutionOutcome, ExecutionStage, SettlementManager,
    SolverFillInfo,
};
pub use orchestrator::{
    BatchResult, ExecutionResult, IntentOrchestrator, IntentStatus, OrchestratorConfig,
    OrchestratorError,
};
pub use recovery::{
    RecoveryAction, RecoveryError, RecoveryManager, RecoveryResult, RecoveryStats, SettlementPhase,
    SettlementState,
};
pub use validator::{IntentValidator, ValidationError};
