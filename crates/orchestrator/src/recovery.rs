use atom_intents_settlement::{EscrowContract, EscrowLock, SolverVaultContract, VaultLock};
use cosmwasm_std::Uint128;
use thiserror::Error;
use tracing::{error, info, warn};

#[cfg(test)]
use async_trait::async_trait;

/// Recovery actions for failed settlements
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Retry with a different solver
    RetryWithDifferentSolver,

    /// Refund user and allow retry
    RefundAndRetry,

    /// Partial settlement completed, refund remainder
    PartialSettlement {
        delivered: Uint128,
        refunded: Uint128,
    },

    /// Requires manual intervention
    ManualIntervention { reason: String },

    /// Slash solver bond for misbehavior
    SlashSolver {
        solver_id: String,
        amount: Uint128,
        reason: String,
    },
}

/// Settlement state for recovery tracking
#[derive(Debug, Clone)]
pub struct SettlementState {
    pub settlement_id: String,
    pub intent_id: String,
    pub solver_id: String,
    pub user_lock: Option<EscrowLock>,
    pub solver_lock: Option<VaultLock>,
    pub status: SettlementPhase,
    pub created_at: u64,
    pub timeout_at: u64,
}

/// Settlement execution phase
#[derive(Debug, Clone, PartialEq)]
pub enum SettlementPhase {
    /// Initial state
    Initiated,

    /// User funds locked
    UserFundsLocked,

    /// Solver funds locked
    BothFundsLocked,

    /// IBC transfer in progress
    TransferInProgress,

    /// IBC transfer completed
    TransferCompleted,

    /// Settlement completed successfully
    Completed,

    /// Settlement failed
    Failed { reason: String },

    /// Settlement timed out
    TimedOut,
}

/// Recovery manager for handling settlement failures
pub struct RecoveryManager<E, V>
where
    E: EscrowContract,
    V: SolverVaultContract,
{
    user_escrow: E,
    solver_vault: V,
    timeout_threshold: u64,
}

impl<E, V> RecoveryManager<E, V>
where
    E: EscrowContract,
    V: SolverVaultContract,
{
    pub fn new(user_escrow: E, solver_vault: V, timeout_threshold: u64) -> Self {
        Self {
            user_escrow,
            solver_vault,
            timeout_threshold,
        }
    }

    /// Check for stuck settlements that need recovery
    pub async fn check_stuck_settlements(
        &self,
        settlements: &[SettlementState],
        current_time: u64,
    ) -> Vec<(String, RecoveryAction)> {
        let mut recovery_actions = Vec::new();

        for settlement in settlements {
            // Check if settlement has timed out
            if current_time >= settlement.timeout_at {
                info!(
                    settlement_id = %settlement.settlement_id,
                    timeout_at = settlement.timeout_at,
                    current_time = current_time,
                    "Settlement timeout detected"
                );

                let action = self.determine_recovery_action(settlement, current_time);
                recovery_actions.push((settlement.settlement_id.clone(), action));
            }
            // Check if settlement is taking too long (warning threshold)
            else if current_time >= settlement.created_at + self.timeout_threshold {
                warn!(
                    settlement_id = %settlement.settlement_id,
                    elapsed = current_time - settlement.created_at,
                    "Settlement taking longer than expected"
                );
            }
        }

        recovery_actions
    }

    /// Determine appropriate recovery action based on settlement state
    pub fn determine_recovery_action(
        &self,
        settlement: &SettlementState,
        _current_time: u64,
    ) -> RecoveryAction {
        match settlement.status {
            SettlementPhase::Initiated | SettlementPhase::UserFundsLocked => {
                // Early phase failure - just refund and retry
                RecoveryAction::RefundAndRetry
            }
            SettlementPhase::BothFundsLocked | SettlementPhase::TransferInProgress => {
                // Funds locked but transfer incomplete - needs investigation
                RecoveryAction::ManualIntervention {
                    reason: format!(
                        "Settlement stuck in phase {:?}, requires investigation",
                        settlement.status
                    ),
                }
            }
            SettlementPhase::TransferCompleted => {
                // Transfer completed but settlement not finalized - likely race condition
                RecoveryAction::ManualIntervention {
                    reason: "Transfer completed but settlement not finalized".to_string(),
                }
            }
            SettlementPhase::TimedOut => {
                // IBC timeout - slash solver if they failed to deliver
                RecoveryAction::SlashSolver {
                    solver_id: settlement.solver_id.clone(),
                    amount: settlement
                        .solver_lock
                        .as_ref()
                        .map(|l| l.amount)
                        .unwrap_or(Uint128::zero()),
                    reason: "IBC transfer timeout".to_string(),
                }
            }
            SettlementPhase::Failed { ref reason } => {
                // Generic failure - determine if solver or system fault
                if reason.contains("solver") {
                    RecoveryAction::RetryWithDifferentSolver
                } else {
                    RecoveryAction::ManualIntervention {
                        reason: reason.clone(),
                    }
                }
            }
            SettlementPhase::Completed => {
                // Already completed - no recovery needed
                RecoveryAction::ManualIntervention {
                    reason: "Settlement already completed".to_string(),
                }
            }
        }
    }

    /// Attempt to recover a stuck settlement
    pub async fn recover_settlement(
        &self,
        settlement: &SettlementState,
        action: RecoveryAction,
    ) -> Result<RecoveryResult, RecoveryError> {
        info!(
            settlement_id = %settlement.settlement_id,
            action = ?action,
            "Attempting settlement recovery"
        );

        match action {
            RecoveryAction::RefundAndRetry => {
                self.refund_user(settlement).await?;
                Ok(RecoveryResult::Refunded {
                    settlement_id: settlement.settlement_id.clone(),
                    user: settlement
                        .user_lock
                        .as_ref()
                        .map(|l| l.owner.clone())
                        .unwrap_or_default(),
                    amount: settlement
                        .user_lock
                        .as_ref()
                        .map(|l| l.amount)
                        .unwrap_or(Uint128::zero()),
                })
            }
            RecoveryAction::RetryWithDifferentSolver => {
                // Unlock solver funds and refund user for retry
                if let Some(ref solver_lock) = settlement.solver_lock {
                    (&self.solver_vault)
                        .unlock(solver_lock)
                        .await
                        .map_err(|e| RecoveryError::UnlockFailed {
                            reason: e.to_string(),
                        })?;
                }
                self.refund_user(settlement).await?;

                Ok(RecoveryResult::ReadyForRetry {
                    settlement_id: settlement.settlement_id.clone(),
                    intent_id: settlement.intent_id.clone(),
                })
            }
            RecoveryAction::SlashSolver {
                solver_id,
                amount,
                reason,
            } => {
                self.slash_solver(settlement, &solver_id, amount, &reason)
                    .await?;
                Ok(RecoveryResult::SolverSlashed {
                    solver_id,
                    amount,
                    reason,
                })
            }
            RecoveryAction::PartialSettlement {
                delivered,
                refunded,
            } => {
                // Partial delivery - refund the undelivered portion
                self.refund_user(settlement).await?;
                Ok(RecoveryResult::PartialRecovery {
                    settlement_id: settlement.settlement_id.clone(),
                    delivered,
                    refunded,
                })
            }
            RecoveryAction::ManualIntervention { reason } => {
                error!(
                    settlement_id = %settlement.settlement_id,
                    reason = %reason,
                    "Manual intervention required"
                );
                Ok(RecoveryResult::RequiresManualIntervention {
                    settlement_id: settlement.settlement_id.clone(),
                    reason,
                })
            }
        }
    }

    /// Refund user's locked funds
    async fn refund_user(&self, settlement: &SettlementState) -> Result<(), RecoveryError> {
        if let Some(ref user_lock) = settlement.user_lock {
            info!(
                settlement_id = %settlement.settlement_id,
                user = %user_lock.owner,
                amount = %user_lock.amount,
                "Refunding user"
            );

            (&self.user_escrow).refund(user_lock).await.map_err(|e| {
                RecoveryError::RefundFailed {
                    reason: e.to_string(),
                }
            })?;

            Ok(())
        } else {
            warn!(
                settlement_id = %settlement.settlement_id,
                "No user lock found, nothing to refund"
            );
            Ok(())
        }
    }

    /// Slash solver's bond for misbehavior
    async fn slash_solver(
        &self,
        settlement: &SettlementState,
        solver_id: &str,
        amount: Uint128,
        reason: &str,
    ) -> Result<(), RecoveryError> {
        info!(
            settlement_id = %settlement.settlement_id,
            solver_id = %solver_id,
            amount = %amount,
            reason = %reason,
            "Slashing solver bond"
        );

        // First refund the user
        self.refund_user(settlement).await?;

        // Then slash the solver bond
        if let Some(ref solver_lock) = settlement.solver_lock {
            // In a real implementation, this would slash the bond rather than just unlock
            // For now, we'll just unlock to return funds
            // TODO: Implement actual slashing mechanism
            (&self.solver_vault)
                .unlock(solver_lock)
                .await
                .map_err(|e| RecoveryError::SlashFailed {
                    reason: e.to_string(),
                })?;
        }

        Ok(())
    }

    /// Get recovery statistics
    pub async fn get_recovery_stats(&self, settlements: &[SettlementState]) -> RecoveryStats {
        let total = settlements.len();
        let completed = settlements
            .iter()
            .filter(|s| s.status == SettlementPhase::Completed)
            .count();
        let failed = settlements
            .iter()
            .filter(|s| matches!(s.status, SettlementPhase::Failed { .. }))
            .count();
        let timed_out = settlements
            .iter()
            .filter(|s| s.status == SettlementPhase::TimedOut)
            .count();
        let in_progress = total - completed - failed - timed_out;

        RecoveryStats {
            total_settlements: total,
            completed,
            failed,
            timed_out,
            in_progress,
        }
    }
}

/// Result of a recovery attempt
#[derive(Debug)]
pub enum RecoveryResult {
    /// User refunded successfully
    Refunded {
        settlement_id: String,
        user: String,
        amount: Uint128,
    },

    /// Ready for retry with different solver
    ReadyForRetry {
        settlement_id: String,
        intent_id: String,
    },

    /// Solver bond slashed
    SolverSlashed {
        solver_id: String,
        amount: Uint128,
        reason: String,
    },

    /// Partial recovery completed
    PartialRecovery {
        settlement_id: String,
        delivered: Uint128,
        refunded: Uint128,
    },

    /// Requires manual intervention
    RequiresManualIntervention {
        settlement_id: String,
        reason: String,
    },
}

/// Recovery statistics
#[derive(Debug, Clone)]
pub struct RecoveryStats {
    pub total_settlements: usize,
    pub completed: usize,
    pub failed: usize,
    pub timed_out: usize,
    pub in_progress: usize,
}

/// Recovery errors
#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("refund failed: {reason}")]
    RefundFailed { reason: String },

    #[error("unlock failed: {reason}")]
    UnlockFailed { reason: String },

    #[error("slash failed: {reason}")]
    SlashFailed { reason: String },

    #[error("settlement not found: {settlement_id}")]
    SettlementNotFound { settlement_id: String },

    #[error("invalid state: {reason}")]
    InvalidState { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_settlement::SettlementError;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    // Mock escrow contract
    struct MockEscrow {
        refund_calls: Arc<RwLock<Vec<String>>>,
    }

    impl MockEscrow {
        fn new() -> Self {
            Self {
                refund_calls: Arc::new(RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl EscrowContract for MockEscrow {
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

        async fn refund(&self, lock: &EscrowLock) -> Result<(), SettlementError> {
            self.refund_calls.write().await.push(lock.id.clone());
            Ok(())
        }
    }

    // Mock vault contract
    struct MockVault {
        unlock_calls: Arc<RwLock<Vec<String>>>,
    }

    impl MockVault {
        fn new() -> Self {
            Self {
                unlock_calls: Arc::new(RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl SolverVaultContract for MockVault {
        async fn lock(
            &self,
            _solver_id: &str,
            _amount: Uint128,
            _denom: &str,
            _timeout: u64,
        ) -> Result<VaultLock, SettlementError> {
            unimplemented!()
        }

        async fn unlock(&self, lock: &VaultLock) -> Result<(), SettlementError> {
            self.unlock_calls.write().await.push(lock.id.clone());
            Ok(())
        }

        async fn mark_complete(&self, _lock: &VaultLock) -> Result<(), SettlementError> {
            unimplemented!()
        }
    }

    fn make_test_settlement(phase: SettlementPhase, timeout_at: u64) -> SettlementState {
        SettlementState {
            settlement_id: "settlement-1".to_string(),
            intent_id: "intent-1".to_string(),
            solver_id: "solver-1".to_string(),
            user_lock: Some(EscrowLock {
                id: "lock-1".to_string(),
                amount: Uint128::new(1_000_000),
                denom: "uatom".to_string(),
                owner: "user1".to_string(),
                expires_at: timeout_at,
            }),
            solver_lock: Some(VaultLock {
                id: "vault-lock-1".to_string(),
                solver_id: "solver-1".to_string(),
                amount: Uint128::new(10_000_000),
                denom: "uusdc".to_string(),
                expires_at: timeout_at,
            }),
            status: phase,
            created_at: 1000,
            timeout_at,
        }
    }

    #[test]
    fn test_recovery_manager_creation() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let _manager = RecoveryManager::new(escrow, vault, 600);
    }

    #[tokio::test]
    async fn test_check_stuck_settlements_timeout() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let manager = RecoveryManager::new(escrow, vault, 600);

        let settlement = make_test_settlement(SettlementPhase::BothFundsLocked, 2000);
        let current_time = 2500; // After timeout

        let actions = manager
            .check_stuck_settlements(&[settlement], current_time)
            .await;

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].0, "settlement-1");
    }

    #[tokio::test]
    async fn test_check_stuck_settlements_no_timeout() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let manager = RecoveryManager::new(escrow, vault, 600);

        let settlement = make_test_settlement(SettlementPhase::BothFundsLocked, 2000);
        let current_time = 1500; // Before timeout

        let actions = manager
            .check_stuck_settlements(&[settlement], current_time)
            .await;

        assert_eq!(actions.len(), 0);
    }

    #[test]
    fn test_determine_recovery_action_early_phase() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let manager = RecoveryManager::new(escrow, vault, 600);

        let settlement = make_test_settlement(SettlementPhase::Initiated, 2000);
        let action = manager.determine_recovery_action(&settlement, 2500);

        assert!(matches!(action, RecoveryAction::RefundAndRetry));
    }

    #[test]
    fn test_determine_recovery_action_timeout() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let manager = RecoveryManager::new(escrow, vault, 600);

        let settlement = make_test_settlement(SettlementPhase::TimedOut, 2000);
        let action = manager.determine_recovery_action(&settlement, 2500);

        assert!(matches!(action, RecoveryAction::SlashSolver { .. }));
    }

    #[tokio::test]
    async fn test_refund_user() {
        let escrow = MockEscrow::new();
        let refund_calls = escrow.refund_calls.clone();
        let vault = MockVault::new();
        let manager = RecoveryManager::new(escrow, vault, 600);

        let settlement = make_test_settlement(
            SettlementPhase::Failed {
                reason: "test".to_string(),
            },
            2000,
        );

        let result = manager.refund_user(&settlement).await;
        assert!(result.is_ok());

        let calls = refund_calls.read().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "lock-1");
    }

    #[tokio::test]
    async fn test_recover_settlement_refund_and_retry() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let manager = RecoveryManager::new(escrow, vault, 600);

        let settlement = make_test_settlement(SettlementPhase::UserFundsLocked, 2000);
        let action = RecoveryAction::RefundAndRetry;

        let result = manager.recover_settlement(&settlement, action).await;
        assert!(result.is_ok());

        match result.unwrap() {
            RecoveryResult::Refunded { settlement_id, .. } => {
                assert_eq!(settlement_id, "settlement-1");
            }
            _ => panic!("Expected Refunded result"),
        }
    }

    #[tokio::test]
    async fn test_get_recovery_stats() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let manager = RecoveryManager::new(escrow, vault, 600);

        let settlements = vec![
            make_test_settlement(SettlementPhase::Completed, 2000),
            make_test_settlement(
                SettlementPhase::Failed {
                    reason: "test".to_string(),
                },
                2000,
            ),
            make_test_settlement(SettlementPhase::TimedOut, 2000),
            make_test_settlement(SettlementPhase::TransferInProgress, 2000),
        ];

        let stats = manager.get_recovery_stats(&settlements).await;

        assert_eq!(stats.total_settlements, 4);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.timed_out, 1);
        assert_eq!(stats.in_progress, 1);
    }
}
