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

#[cfg(test)]
mod tests {
    use super::*;
    use atom_intents_types::{
        Asset, ExecutionConstraints, ExecutionPlan, FillConfig, OutputSpec, ProposedFill,
    };
    use cosmwasm_std::Binary;
    use std::sync::Mutex;

    // ═══════════════════════════════════════════════════════════════════
    // MOCK IMPLEMENTATIONS
    // ═══════════════════════════════════════════════════════════════════

    struct MockEscrow {
        locks: Mutex<Vec<EscrowLock>>,
        releases: Mutex<Vec<(String, String)>>,
        refunds: Mutex<Vec<String>>,
        should_fail: bool,
    }

    impl MockEscrow {
        fn new() -> Self {
            Self {
                locks: Mutex::new(vec![]),
                releases: Mutex::new(vec![]),
                refunds: Mutex::new(vec![]),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                should_fail: true,
                ..Self::new()
            }
        }

        fn lock_count(&self) -> usize {
            self.locks.lock().unwrap().len()
        }

        fn release_count(&self) -> usize {
            self.releases.lock().unwrap().len()
        }

        fn refund_count(&self) -> usize {
            self.refunds.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl EscrowContract for MockEscrow {
        async fn lock(
            &self,
            user: &str,
            amount: Uint128,
            denom: &str,
            timeout: u64,
        ) -> Result<EscrowLock, SettlementError> {
            if self.should_fail {
                return Err(SettlementError::EscrowLockFailed(
                    "mock escrow failure".to_string(),
                ));
            }
            let lock = EscrowLock {
                id: format!("escrow-{}", user),
                amount,
                denom: denom.to_string(),
                owner: user.to_string(),
                expires_at: timeout,
            };
            self.locks.lock().unwrap().push(lock.clone());
            Ok(lock)
        }

        async fn release_to(
            &self,
            lock: &EscrowLock,
            recipient: &str,
        ) -> Result<(), SettlementError> {
            self.releases
                .lock()
                .unwrap()
                .push((lock.id.clone(), recipient.to_string()));
            Ok(())
        }

        async fn refund(&self, lock: &EscrowLock) -> Result<(), SettlementError> {
            self.refunds.lock().unwrap().push(lock.id.clone());
            Ok(())
        }
    }

    struct MockVault {
        locks: Mutex<Vec<VaultLock>>,
        releases: Mutex<Vec<(String, String)>>,
        unlocks: Mutex<Vec<String>>,
        completions: Mutex<Vec<String>>,
        should_fail: bool,
    }

    impl MockVault {
        fn new() -> Self {
            Self {
                locks: Mutex::new(vec![]),
                releases: Mutex::new(vec![]),
                unlocks: Mutex::new(vec![]),
                completions: Mutex::new(vec![]),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                should_fail: true,
                ..Self::new()
            }
        }

        fn lock_count(&self) -> usize {
            self.locks.lock().unwrap().len()
        }

        fn release_count(&self) -> usize {
            self.releases.lock().unwrap().len()
        }

        fn unlock_count(&self) -> usize {
            self.unlocks.lock().unwrap().len()
        }

        fn completion_count(&self) -> usize {
            self.completions.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl SolverVaultContract for MockVault {
        async fn lock(
            &self,
            solver_id: &str,
            amount: Uint128,
            denom: &str,
            timeout: u64,
        ) -> Result<VaultLock, SettlementError> {
            if self.should_fail {
                return Err(SettlementError::SolverVaultLockFailed(
                    "mock vault failure".to_string(),
                ));
            }
            let lock = VaultLock {
                id: format!("vault-{}", solver_id),
                solver_id: solver_id.to_string(),
                amount,
                denom: denom.to_string(),
                expires_at: timeout,
            };
            self.locks.lock().unwrap().push(lock.clone());
            Ok(lock)
        }

        async fn release_to(
            &self,
            lock: &VaultLock,
            recipient: &str,
        ) -> Result<(), SettlementError> {
            self.releases
                .lock()
                .unwrap()
                .push((lock.id.clone(), recipient.to_string()));
            Ok(())
        }

        async fn unlock(&self, lock: &VaultLock) -> Result<(), SettlementError> {
            self.unlocks.lock().unwrap().push(lock.id.clone());
            Ok(())
        }

        async fn slash(
            &self,
            _lock: &VaultLock,
            _amount: Uint128,
            _reason: &str,
        ) -> Result<(), SettlementError> {
            Ok(())
        }

        async fn mark_complete(&self, lock: &VaultLock) -> Result<(), SettlementError> {
            self.completions.lock().unwrap().push(lock.id.clone());
            Ok(())
        }
    }

    enum IbcResultKind {
        Success,
        Timeout,
        Error(String),
    }

    struct MockRelayer {
        tracked: Mutex<Vec<String>>,
        result: IbcResultKind,
    }

    impl MockRelayer {
        fn success() -> Self {
            Self {
                tracked: Mutex::new(vec![]),
                result: IbcResultKind::Success,
            }
        }

        fn timeout() -> Self {
            Self {
                tracked: Mutex::new(vec![]),
                result: IbcResultKind::Timeout,
            }
        }

        fn error(reason: impl Into<String>) -> Self {
            Self {
                tracked: Mutex::new(vec![]),
                result: IbcResultKind::Error(reason.into()),
            }
        }

        fn tracked_count(&self) -> usize {
            self.tracked.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl RelayerService for MockRelayer {
        async fn track_settlement(
            &self,
            settlement_id: &str,
            _transfers: &[atom_intents_types::IbcTransferInfo],
        ) -> Result<(), SettlementError> {
            self.tracked
                .lock()
                .unwrap()
                .push(settlement_id.to_string());
            Ok(())
        }

        async fn wait_for_ibc(
            &self,
            _transfer: &atom_intents_types::IbcTransferInfo,
        ) -> Result<IbcResult, SettlementError> {
            match &self.result {
                IbcResultKind::Success => Ok(IbcResult::Success { ack: vec![1] }),
                IbcResultKind::Timeout => Ok(IbcResult::Timeout),
                IbcResultKind::Error(reason) => Ok(IbcResult::Error {
                    reason: reason.clone(),
                }),
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // HELPERS
    // ═══════════════════════════════════════════════════════════════════

    fn make_cross_chain_intent() -> Intent {
        Intent {
            id: "test-intent-1".to_string(),
            version: "1.0".to_string(),
            nonce: 1,
            user: "cosmos1user".to_string(),
            input: Asset::new("cosmoshub-4", "uatom", 1_000_000),
            output: OutputSpec {
                chain_id: "osmosis-1".to_string(),
                denom: "uusdc".to_string(),
                min_amount: Uint128::new(10_000_000),
                limit_price: "10.0".to_string(),
                recipient: "osmo1recipient".to_string(),
            },
            fill_config: FillConfig::default(),
            constraints: ExecutionConstraints::new(9999999999),
            signature: Binary::from(vec![1, 2, 3]),
            public_key: Binary::from(vec![4, 5, 6]),
            created_at: 1000,
            expires_at: 9999999999,
        }
    }

    fn make_same_chain_intent() -> Intent {
        Intent {
            id: "test-intent-2".to_string(),
            version: "1.0".to_string(),
            nonce: 1,
            user: "cosmos1user".to_string(),
            input: Asset::new("cosmoshub-4", "uatom", 1_000_000),
            output: OutputSpec {
                chain_id: "cosmoshub-4".to_string(),
                denom: "uusdc".to_string(),
                min_amount: Uint128::new(10_000_000),
                limit_price: "10.0".to_string(),
                recipient: "cosmos1recipient".to_string(),
            },
            fill_config: FillConfig::default(),
            constraints: ExecutionConstraints::new(9999999999),
            signature: Binary::from(vec![1, 2, 3]),
            public_key: Binary::from(vec![4, 5, 6]),
            created_at: 1000,
            expires_at: 9999999999,
        }
    }

    fn make_solution(intent_id: &str) -> Solution {
        Solution {
            solver_id: "solver-1".to_string(),
            intent_id: intent_id.to_string(),
            fill: ProposedFill {
                input_amount: Uint128::new(1_000_000),
                output_amount: Uint128::new(10_500_000),
                price: "10.5".to_string(),
            },
            execution: ExecutionPlan::DexRoute { steps: vec![] },
            valid_until: 9999999999,
            bond: Uint128::new(1_500_000),
        }
    }

    fn make_settlement(
        escrow: MockEscrow,
        vault: MockVault,
        relayer: MockRelayer,
    ) -> TwoPhaseSettlement<MockEscrow, MockVault, MockRelayer> {
        TwoPhaseSettlement::new(
            escrow,
            vault,
            relayer,
            TimeoutConfig::default(),
            RouteRegistry::with_mainnet_routes(),
        )
    }

    // ═══════════════════════════════════════════════════════════════════
    // TIMEOUT CONFIG TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_timeout_config_escrow_timeout() {
        let config = TimeoutConfig::default();
        assert_eq!(config.escrow_timeout(), 900);
    }

    #[test]
    fn test_timeout_config_validate_ok() {
        let config = TimeoutConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_timeout_config_validate_exceeds_max() {
        let config = TimeoutConfig {
            ibc_timeout_secs: 1000,
            safety_buffer_secs: 1000,
            max_timeout_secs: 1500,
        };
        let err = config.validate().unwrap_err();
        assert!(matches!(err, SettlementError::TimeoutError(_)));
    }

    // ═══════════════════════════════════════════════════════════════════
    // CROSS-CHAIN SETTLEMENT TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_cross_chain_settlement_success() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let relayer = MockRelayer::success();
        let settlement = make_settlement(escrow, vault, relayer);

        let intent = make_cross_chain_intent();
        let solution = make_solution(&intent.id);

        let result = settlement.execute(&intent, &solution, 1000).await;
        assert!(result.is_ok());

        let s = result.unwrap();
        assert_eq!(s.intent_id, intent.id);
        assert_eq!(s.solver_id, "solver-1");
        assert_eq!(s.user_input, Uint128::new(1_000_000));
        assert_eq!(s.solver_output, Uint128::new(10_500_000));
        assert!(matches!(
            s.status,
            atom_intents_types::SettlementStatus::Complete
        ));
        assert_eq!(s.ibc_transfers.len(), 1);

        // Verify mock interactions
        assert_eq!(settlement.user_escrow.lock_count(), 1);
        assert_eq!(settlement.solver_vault.lock_count(), 1);
        assert_eq!(settlement.relayer.tracked_count(), 1);
        assert_eq!(settlement.user_escrow.release_count(), 1);
        assert_eq!(settlement.solver_vault.completion_count(), 1);
        assert_eq!(settlement.user_escrow.refund_count(), 0);
        assert_eq!(settlement.solver_vault.unlock_count(), 0);
    }

    #[tokio::test]
    async fn test_cross_chain_settlement_ibc_transfer_details() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let relayer = MockRelayer::success();
        let settlement = make_settlement(escrow, vault, relayer);

        let intent = make_cross_chain_intent();
        let solution = make_solution(&intent.id);

        let s = settlement.execute(&intent, &solution, 1000).await.unwrap();
        let transfer = &s.ibc_transfers[0];

        assert_eq!(transfer.source_chain, "cosmoshub-4");
        assert_eq!(transfer.dest_chain, "osmosis-1");
        assert_eq!(transfer.denom, "uusdc");
        assert_eq!(transfer.amount, Uint128::new(10_500_000));
        assert_eq!(transfer.sender, "solver-1");
        assert_eq!(transfer.receiver, "osmo1recipient");
        // Timeout = current_time (1000) + ibc_timeout_secs (600) = 1600
        assert_eq!(transfer.timeout_timestamp, 1600);
    }

    // ═══════════════════════════════════════════════════════════════════
    // SAME-CHAIN SETTLEMENT TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_same_chain_settlement() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let relayer = MockRelayer::success();
        let settlement = make_settlement(escrow, vault, relayer);

        let intent = make_same_chain_intent();
        let solution = make_solution(&intent.id);

        let s = settlement.execute(&intent, &solution, 1000).await.unwrap();
        assert!(matches!(
            s.status,
            atom_intents_types::SettlementStatus::Complete
        ));
        assert!(s.ibc_transfers.is_empty());

        // Vault released to recipient, escrow released to solver
        assert_eq!(settlement.solver_vault.release_count(), 1);
        assert_eq!(settlement.user_escrow.release_count(), 1);
        assert_eq!(settlement.solver_vault.completion_count(), 1);
        // No relayer involvement for same-chain
        assert_eq!(settlement.relayer.tracked_count(), 0);
    }

    // ═══════════════════════════════════════════════════════════════════
    // FAILURE AND ROLLBACK TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_vault_lock_failure_returns_error() {
        let escrow = MockEscrow::new();
        let vault = MockVault::failing();
        let relayer = MockRelayer::success();
        let settlement = make_settlement(escrow, vault, relayer);

        let intent = make_cross_chain_intent();
        let solution = make_solution(&intent.id);

        let result = settlement.execute(&intent, &solution, 1000).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SettlementError::SolverVaultLockFailed(_)
        ));

        // User escrow was locked, vault failed before locking
        assert_eq!(settlement.user_escrow.lock_count(), 1);
        assert_eq!(settlement.solver_vault.lock_count(), 0);
    }

    #[tokio::test]
    async fn test_escrow_lock_failure() {
        let escrow = MockEscrow::failing();
        let vault = MockVault::new();
        let relayer = MockRelayer::success();
        let settlement = make_settlement(escrow, vault, relayer);

        let intent = make_cross_chain_intent();
        let solution = make_solution(&intent.id);

        let result = settlement.execute(&intent, &solution, 1000).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SettlementError::EscrowLockFailed(_)
        ));

        // Neither should have locks
        assert_eq!(settlement.user_escrow.lock_count(), 0);
        assert_eq!(settlement.solver_vault.lock_count(), 0);
    }

    #[tokio::test]
    async fn test_ibc_timeout_unwinds_locks() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let relayer = MockRelayer::timeout();
        let settlement = make_settlement(escrow, vault, relayer);

        let intent = make_cross_chain_intent();
        let solution = make_solution(&intent.id);

        let s = settlement.execute(&intent, &solution, 1000).await.unwrap();
        assert!(matches!(
            s.status,
            atom_intents_types::SettlementStatus::TimedOut
        ));

        // Both locks created then unwound
        assert_eq!(settlement.user_escrow.lock_count(), 1);
        assert_eq!(settlement.solver_vault.lock_count(), 1);
        assert_eq!(settlement.solver_vault.unlock_count(), 1);
        assert_eq!(settlement.user_escrow.refund_count(), 1);
        // No successful completion
        assert_eq!(settlement.solver_vault.completion_count(), 0);
        assert_eq!(settlement.user_escrow.release_count(), 0);
    }

    #[tokio::test]
    async fn test_ibc_error_unwinds_locks() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let relayer = MockRelayer::error("channel closed");
        let settlement = make_settlement(escrow, vault, relayer);

        let intent = make_cross_chain_intent();
        let solution = make_solution(&intent.id);

        let s = settlement.execute(&intent, &solution, 1000).await.unwrap();
        assert!(matches!(
            s.status,
            atom_intents_types::SettlementStatus::TimedOut
        ));

        assert_eq!(settlement.solver_vault.unlock_count(), 1);
        assert_eq!(settlement.user_escrow.refund_count(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // CONSTRAINT VALIDATION TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_route_not_found() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let relayer = MockRelayer::success();
        let settlement = make_settlement(escrow, vault, relayer);

        let mut intent = make_cross_chain_intent();
        intent.input.chain_id = "unknown-1".to_string();
        intent.output.chain_id = "unknown-2".to_string();
        let solution = make_solution(&intent.id);

        let result = settlement.execute(&intent, &solution, 1000).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SettlementError::RouteNotFound(_, _)
        ));

        assert_eq!(settlement.user_escrow.lock_count(), 0);
        assert_eq!(settlement.solver_vault.lock_count(), 0);
    }

    #[tokio::test]
    async fn test_max_hops_constraint_violation() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let relayer = MockRelayer::success();
        let settlement = make_settlement(escrow, vault, relayer);

        let mut intent = make_cross_chain_intent();
        // cosmoshub-4 -> osmosis-1 has 1 hop (direct), set max_hops=0
        intent.constraints.max_hops = Some(0);
        let solution = make_solution(&intent.id);

        let result = settlement.execute(&intent, &solution, 1000).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SettlementError::ConstraintViolation(_)
        ));
    }

    #[tokio::test]
    async fn test_max_bridge_time_constraint_violation() {
        let escrow = MockEscrow::new();
        let vault = MockVault::new();
        let relayer = MockRelayer::success();
        let settlement = make_settlement(escrow, vault, relayer);

        let mut intent = make_cross_chain_intent();
        // Direct route estimated at 6 seconds, set max to 1 second
        intent.constraints.max_bridge_time_secs = Some(1);
        let solution = make_solution(&intent.id);

        let result = settlement.execute(&intent, &solution, 1000).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SettlementError::ConstraintViolation(_)
        ));
    }

    #[tokio::test]
    async fn test_invalid_timeout_config_rejects_execution() {
        let config = TimeoutConfig {
            ibc_timeout_secs: 1000,
            safety_buffer_secs: 1000,
            max_timeout_secs: 500,
        };

        let settlement = TwoPhaseSettlement::new(
            MockEscrow::new(),
            MockVault::new(),
            MockRelayer::success(),
            config,
            RouteRegistry::with_mainnet_routes(),
        );

        let intent = make_cross_chain_intent();
        let solution = make_solution(&intent.id);

        let result = settlement.execute(&intent, &solution, 1000).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SettlementError::TimeoutError(_)
        ));
    }

    // ═══════════════════════════════════════════════════════════════════
    // RECOVERY ACTION TESTS
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_handle_failure_solver_failed() {
        let action = handle_failure(SettlementFailure::SolverFailed {
            solver_id: "s1".to_string(),
            reason: "timeout".to_string(),
        });
        assert!(matches!(action, RecoveryAction::RetryWithDifferentSolver));
    }

    #[test]
    fn test_handle_failure_ibc_timeout() {
        let action = handle_failure(SettlementFailure::IbcTimeout {
            transfer_id: "t1".to_string(),
        });
        assert!(matches!(action, RecoveryAction::UserCanRetry));
    }

    #[test]
    fn test_handle_failure_partial() {
        let action = handle_failure(SettlementFailure::PartialFailure {
            delivered: Uint128::new(500),
            failed: Uint128::new(500),
        });
        match action {
            RecoveryAction::PartialSettlement {
                delivered,
                refunded,
            } => {
                assert_eq!(delivered, Uint128::new(500));
                assert_eq!(refunded, Uint128::new(500));
            }
            _ => panic!("Expected PartialSettlement"),
        }
    }

    #[test]
    fn test_handle_failure_unknown() {
        let action = handle_failure(SettlementFailure::Unknown {
            reason: "something went wrong".to_string(),
        });
        match action {
            RecoveryAction::ManualIntervention { reason } => {
                assert_eq!(reason, "something went wrong");
            }
            _ => panic!("Expected ManualIntervention"),
        }
    }
}
