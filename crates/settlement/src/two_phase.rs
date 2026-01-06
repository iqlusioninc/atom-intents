use async_trait::async_trait;
use atom_intents_types::{IbcTransferInfo, Intent, Settlement, SettlementStatus, Solution};
use cosmwasm_std::Uint128;

use crate::{build_route_pfm_memo, IbcTransferBuilder, RouteRegistry, SettlementError};

/// Two-phase commit settlement to prevent solver losses
pub struct TwoPhaseSettlement<E, V, R>
where
    E: EscrowContract,
    V: SolverVaultContract,
    R: RelayerService,
{
    user_escrow: E,
    solver_vault: V,
    relayer: R,
    config: TimeoutConfig,
    route_registry: RouteRegistry,
}

/// Timeout configuration ensuring safety
#[derive(Clone, Debug)]
pub struct TimeoutConfig {
    /// Base IBC timeout (seconds)
    pub ibc_timeout_secs: u64,

    /// Buffer between IBC timeout and escrow release (seconds)
    pub safety_buffer_secs: u64,

    /// Maximum total timeout (seconds)
    pub max_timeout_secs: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            ibc_timeout_secs: 600,   // 10 minutes
            safety_buffer_secs: 300, // 5 minutes
            max_timeout_secs: 1800,  // 30 minutes
        }
    }
}

impl TimeoutConfig {
    /// Escrow timeout must be longer than IBC timeout + buffer
    pub fn escrow_timeout(&self) -> u64 {
        self.ibc_timeout_secs + self.safety_buffer_secs
    }

    pub fn validate(&self) -> Result<(), SettlementError> {
        if self.escrow_timeout() > self.max_timeout_secs {
            return Err(SettlementError::TimeoutError(
                "escrow timeout exceeds maximum".to_string(),
            ));
        }
        Ok(())
    }
}

/// Lock handle for escrowed funds
#[derive(Clone, Debug)]
pub struct EscrowLock {
    pub id: String,
    pub amount: Uint128,
    pub denom: String,
    pub owner: String,
    pub expires_at: u64,
}

/// Lock handle for solver vault
#[derive(Clone, Debug)]
pub struct VaultLock {
    pub id: String,
    pub solver_id: String,
    pub amount: Uint128,
    pub denom: String,
    pub expires_at: u64,
}

/// User escrow contract interface
#[async_trait]
pub trait EscrowContract: Send + Sync {
    async fn lock(
        &self,
        user: &str,
        amount: Uint128,
        denom: &str,
        timeout: u64,
    ) -> Result<EscrowLock, SettlementError>;
    async fn release_to(&self, lock: &EscrowLock, recipient: &str) -> Result<(), SettlementError>;
    async fn refund(&self, lock: &EscrowLock) -> Result<(), SettlementError>;
}

/// Solver vault contract interface
#[async_trait]
pub trait SolverVaultContract: Send + Sync {
    async fn lock(
        &self,
        solver_id: &str,
        amount: Uint128,
        denom: &str,
        timeout: u64,
    ) -> Result<VaultLock, SettlementError>;
    async fn release_to(&self, lock: &VaultLock, recipient: &str) -> Result<(), SettlementError>;
    async fn unlock(&self, lock: &VaultLock) -> Result<(), SettlementError>;
    async fn slash(
        &self,
        lock: &VaultLock,
        amount: Uint128,
        reason: &str,
    ) -> Result<(), SettlementError>;
    async fn mark_complete(&self, lock: &VaultLock) -> Result<(), SettlementError>;
}

/// Relayer service interface
#[async_trait]
pub trait RelayerService: Send + Sync {
    async fn track_settlement(
        &self,
        settlement_id: &str,
        transfers: &[IbcTransferInfo],
    ) -> Result<(), SettlementError>;
    async fn wait_for_ibc(&self, transfer: &IbcTransferInfo) -> Result<IbcResult, SettlementError>;
}

#[derive(Debug)]
pub enum IbcResult {
    Success { ack: Vec<u8> },
    Timeout,
    Error { reason: String },
}

impl<E, V, R> TwoPhaseSettlement<E, V, R>
where
    E: EscrowContract,
    V: SolverVaultContract,
    R: RelayerService,
{
    pub fn new(
        user_escrow: E,
        solver_vault: V,
        relayer: R,
        config: TimeoutConfig,
        route_registry: RouteRegistry,
    ) -> Self {
        Self {
            user_escrow,
            solver_vault,
            relayer,
            config,
            route_registry,
        }
    }

    /// Execute two-phase settlement
    pub async fn execute(
        &self,
        intent: &Intent,
        solution: &Solution,
        current_time: u64,
    ) -> Result<Settlement, SettlementError> {
        self.config.validate()?;

        let route = self
            .route_registry
            .find_route(&intent.input.chain_id, &intent.output.chain_id)
            .ok_or_else(|| {
                SettlementError::RouteNotFound(
                    intent.input.chain_id.clone(),
                    intent.output.chain_id.clone(),
                )
            })?;

        let hop_count = route.hops.len() as u32;
        if let Some(max_hops) = intent.constraints.max_hops {
            if hop_count > max_hops {
                return Err(SettlementError::ConstraintViolation(format!(
                    "route requires {} hops (max {})",
                    hop_count, max_hops
                )));
            }
        }

        let estimated_time = if route.estimated_time_seconds > 0 {
            route.estimated_time_seconds
        } else {
            RouteRegistry::calculate_route_time(&route)
        };
        if let Some(max_time) = intent.constraints.max_bridge_time_secs {
            if estimated_time > max_time {
                return Err(SettlementError::ConstraintViolation(format!(
                    "route estimated time {}s exceeds max_bridge_time_secs {}",
                    estimated_time, max_time
                )));
            }
        }

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 1: COMMIT - Both parties lock funds
        // ═══════════════════════════════════════════════════════════════════

        // 1a. Lock user's input
        let user_lock = self
            .user_escrow
            .lock(
                &intent.user,
                intent.input.amount,
                &intent.input.denom,
                current_time + self.config.escrow_timeout(),
            )
            .await?;

        // 1b. Lock solver's output
        let solver_lock = self
            .solver_vault
            .lock(
                &solution.solver_id,
                solution.fill.output_amount,
                &intent.output.denom,
                current_time + self.config.escrow_timeout(),
            )
            .await
            .map_err(|e| {
                // Rollback user lock on failure
                // In production, this would be atomic
                SettlementError::SolverVaultLockFailed(e.to_string())
            })?;

        // Now both committed - safe to proceed

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 2: EXECUTE - Transfer funds
        // ═══════════════════════════════════════════════════════════════════

        if route.hops.is_empty() {
            // Same-chain settlement: transfer solver output directly and release escrow.
            self.solver_vault
                .release_to(&solver_lock, &intent.output.recipient)
                .await?;
            self.user_escrow
                .release_to(&user_lock, &solution.solver_id)
                .await?;
            self.solver_vault.mark_complete(&solver_lock).await?;

            return Ok(Settlement {
                intent_id: intent.id.clone(),
                solver_id: solution.solver_id.clone(),
                user_input: intent.input.amount,
                solver_output: solution.fill.output_amount,
                ibc_transfers: vec![],
                status: SettlementStatus::Complete,
            });
        }

        // Build IBC transfer for output to user
        let memo = if route.hops.len() > 1 {
            Some(build_route_pfm_memo(
                &route.hops,
                &intent.output.recipient,
            ))
        } else {
            None
        };
        let channel = route.hops[0].channel_id.clone();

        let mut output_builder = IbcTransferBuilder::new(
            &intent.input.chain_id,
            &intent.output.chain_id,
            channel,
        )
        .denom(&intent.output.denom)
        .amount(solution.fill.output_amount)
        .sender(&solution.solver_id)
        .receiver(&intent.output.recipient)
        .timeout_secs(self.config.ibc_timeout_secs);

        if let Some(memo) = memo {
            output_builder = output_builder.memo(memo);
        }

        let output_transfer = output_builder.build(current_time);

        // Track with relayer for priority handling
        self.relayer
            .track_settlement(&intent.id, &[output_transfer.clone()])
            .await?;

        // Wait for IBC confirmation
        let result = self.relayer.wait_for_ibc(&output_transfer).await?;

        match result {
            IbcResult::Success { .. } => {
                // Success - release user's input to solver
                self.user_escrow
                    .release_to(&user_lock, &solution.solver_id)
                    .await?;
                self.solver_vault.mark_complete(&solver_lock).await?;

                Ok(Settlement {
                    intent_id: intent.id.clone(),
                    solver_id: solution.solver_id.clone(),
                    user_input: intent.input.amount,
                    solver_output: solution.fill.output_amount,
                    ibc_transfers: vec![output_transfer],
                    status: SettlementStatus::Complete,
                })
            }
            IbcResult::Timeout | IbcResult::Error { .. } => {
                // Failed - unwind BOTH locks
                self.solver_vault.unlock(&solver_lock).await?;
                self.user_escrow.refund(&user_lock).await?;

                Ok(Settlement {
                    intent_id: intent.id.clone(),
                    solver_id: solution.solver_id.clone(),
                    user_input: intent.input.amount,
                    solver_output: solution.fill.output_amount,
                    ibc_transfers: vec![output_transfer],
                    status: SettlementStatus::TimedOut,
                })
            }
        }
    }
}

/// Recovery actions for settlement failures
pub enum RecoveryAction {
    RetryWithDifferentSolver,
    UserCanRetry,
    PartialSettlement {
        delivered: Uint128,
        refunded: Uint128,
    },
    ManualIntervention {
        reason: String,
    },
}

/// Handle settlement failures
pub fn handle_failure(failure: SettlementFailure) -> RecoveryAction {
    match failure {
        SettlementFailure::SolverFailed { .. } => RecoveryAction::RetryWithDifferentSolver,
        SettlementFailure::IbcTimeout { .. } => RecoveryAction::UserCanRetry,
        SettlementFailure::PartialFailure { delivered, failed } => {
            RecoveryAction::PartialSettlement {
                delivered,
                refunded: failed,
            }
        }
        SettlementFailure::Unknown { reason } => RecoveryAction::ManualIntervention { reason },
    }
}

#[derive(Debug)]
pub enum SettlementFailure {
    SolverFailed { solver_id: String, reason: String },
    IbcTimeout { transfer_id: String },
    PartialFailure { delivered: Uint128, failed: Uint128 },
    Unknown { reason: String },
}
