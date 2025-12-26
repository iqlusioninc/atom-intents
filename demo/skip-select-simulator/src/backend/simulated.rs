//! Simulated execution backend
//!
//! This backend simulates all blockchain operations in-memory,
//! providing fast, predictable behavior for demos and UI testing.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use rand::Rng;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info};
use uuid::Uuid;

use super::{
    BackendError, BackendEvent, BackendMode, ContractAddresses, EscrowLockResult,
    ExecutionBackend, SettlementResult,
};
use crate::models::{Settlement, SettlementStatus};

/// Simulated escrow lock
#[derive(Debug, Clone)]
struct SimulatedEscrow {
    id: String,
    settlement_id: String,
    user: String,
    amount: u128,
    denom: String,
    locked_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
    released: bool,
    refunded: bool,
}

/// Simulated execution backend
pub struct SimulatedBackend {
    /// In-memory escrow storage
    escrows: Arc<RwLock<HashMap<String, SimulatedEscrow>>>,
    /// Event broadcaster
    event_tx: broadcast::Sender<BackendEvent>,
    /// Simulated latency range (ms)
    latency_range: (u64, u64),
    /// Success rate (0.0 to 1.0)
    success_rate: f64,
}

impl SimulatedBackend {
    /// Create a new simulated backend with default settings
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(256);

        Self {
            escrows: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            latency_range: (100, 300),
            success_rate: 0.95,
        }
    }

    /// Create with custom settings
    pub fn with_settings(latency_range: (u64, u64), success_rate: f64) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        Self {
            escrows: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            latency_range,
            success_rate,
        }
    }

    /// Simulate network latency
    async fn simulate_latency(&self) {
        let delay = {
            let mut rng = rand::thread_rng();
            rng.gen_range(self.latency_range.0..=self.latency_range.1)
        };
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

    /// Check if operation should succeed based on success rate
    fn should_succeed(&self) -> bool {
        let mut rng = rand::thread_rng();
        rng.gen::<f64>() < self.success_rate
    }

    /// Emit an event
    fn emit(&self, event: BackendEvent) {
        let _ = self.event_tx.send(event);
    }
}

impl Default for SimulatedBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionBackend for SimulatedBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Simulated
    }

    async fn lock_escrow(
        &self,
        settlement_id: &str,
        user: &str,
        amount: u128,
        denom: &str,
        timeout_secs: u64,
    ) -> Result<EscrowLockResult, BackendError> {
        self.simulate_latency().await;

        let escrow_id = format!("escrow_{}", Uuid::new_v4());
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(timeout_secs as i64);

        let escrow = SimulatedEscrow {
            id: escrow_id.clone(),
            settlement_id: settlement_id.to_string(),
            user: user.to_string(),
            amount,
            denom: denom.to_string(),
            locked_at: now,
            expires_at,
            released: false,
            refunded: false,
        };

        {
            let mut escrows = self.escrows.write().await;
            escrows.insert(escrow_id.clone(), escrow);
        }

        info!(
            escrow_id = %escrow_id,
            amount = amount,
            denom = %denom,
            "Simulated escrow locked"
        );

        // Emit event
        self.emit(BackendEvent::EscrowLocked {
            settlement_id: settlement_id.to_string(),
            escrow_id: escrow_id.clone(),
            tx_hash: None, // No tx hash in simulation
            block_height: None,
            amount,
            denom: denom.to_string(),
        });

        Ok(EscrowLockResult {
            id: escrow_id,
            tx_hash: None,
            block_height: None,
            amount,
            denom: denom.to_string(),
        })
    }

    async fn release_escrow(
        &self,
        escrow_id: &str,
        recipient: &str,
    ) -> Result<Option<String>, BackendError> {
        self.simulate_latency().await;

        let mut escrows = self.escrows.write().await;
        let escrow = escrows
            .get_mut(escrow_id)
            .ok_or_else(|| BackendError::NotFound(format!("Escrow {} not found", escrow_id)))?;

        if escrow.released {
            return Err(BackendError::EscrowLockFailed("Already released".to_string()));
        }
        if escrow.refunded {
            return Err(BackendError::EscrowLockFailed("Already refunded".to_string()));
        }

        escrow.released = true;

        info!(
            escrow_id = %escrow_id,
            recipient = %recipient,
            amount = escrow.amount,
            "Simulated escrow released"
        );

        Ok(None) // No tx hash in simulation
    }

    async fn refund_escrow(&self, escrow_id: &str) -> Result<Option<String>, BackendError> {
        self.simulate_latency().await;

        let mut escrows = self.escrows.write().await;
        let escrow = escrows
            .get_mut(escrow_id)
            .ok_or_else(|| BackendError::NotFound(format!("Escrow {} not found", escrow_id)))?;

        if escrow.released {
            return Err(BackendError::EscrowLockFailed("Already released".to_string()));
        }
        if escrow.refunded {
            return Err(BackendError::EscrowLockFailed("Already refunded".to_string()));
        }

        escrow.refunded = true;

        info!(
            escrow_id = %escrow_id,
            user = %escrow.user,
            amount = escrow.amount,
            "Simulated escrow refunded"
        );

        Ok(None) // No tx hash in simulation
    }

    async fn execute_settlement(
        &self,
        settlement: &Settlement,
    ) -> Result<SettlementResult, BackendError> {
        debug!(
            settlement_id = %settlement.id,
            phase = ?settlement.phase,
            "Executing simulated settlement"
        );

        // Simulate processing each phase
        self.simulate_latency().await;

        // Emit solver committed event
        self.emit(BackendEvent::SolverCommitted {
            settlement_id: settlement.id.clone(),
            solver_id: settlement.solver_id.clone(),
            tx_hash: None,
        });

        self.simulate_latency().await;

        // Emit IBC started event
        self.emit(BackendEvent::IbcTransferStarted {
            settlement_id: settlement.id.clone(),
            packet_sequence: None,
            tx_hash: None,
        });

        self.simulate_latency().await;

        // Check if this settlement should succeed
        if self.should_succeed() {
            // Emit completion event
            self.emit(BackendEvent::SettlementComplete {
                settlement_id: settlement.id.clone(),
                tx_hash: None,
                output_delivered: settlement.output_amount,
            });

            info!(
                settlement_id = %settlement.id,
                output_amount = settlement.output_amount,
                "Simulated settlement completed successfully"
            );

            Ok(SettlementResult {
                id: settlement.id.clone(),
                status: SettlementStatus::Completed,
                tx_hash: None,
                block_height: None,
                explorer_url: None,
            })
        } else {
            // Emit failure event
            self.emit(BackendEvent::SettlementFailed {
                settlement_id: settlement.id.clone(),
                reason: "Simulated IBC timeout".to_string(),
                recoverable: true,
            });

            info!(
                settlement_id = %settlement.id,
                "Simulated settlement failed (IBC timeout)"
            );

            Ok(SettlementResult {
                id: settlement.id.clone(),
                status: SettlementStatus::Failed,
                tx_hash: None,
                block_height: None,
                explorer_url: None,
            })
        }
    }

    async fn get_settlement_status(
        &self,
        settlement_id: &str,
    ) -> Result<SettlementStatus, BackendError> {
        // In simulated mode, we don't track settlement status separately
        // The AppState handles this
        debug!(
            settlement_id = %settlement_id,
            "Querying simulated settlement status"
        );
        Err(BackendError::NotFound(
            "Use AppState for settlement status in simulated mode".to_string(),
        ))
    }

    fn contract_addresses(&self) -> Option<ContractAddresses> {
        // No contracts in simulated mode
        None
    }

    fn subscribe(&self) -> broadcast::Receiver<BackendEvent> {
        self.event_tx.subscribe()
    }

    async fn health_check(&self) -> Result<bool, BackendError> {
        // Simulated backend is always healthy
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulated_escrow_lifecycle() {
        let backend = SimulatedBackend::with_settings((10, 20), 1.0);

        // Lock escrow
        let result = backend
            .lock_escrow("settlement_1", "cosmos1user...", 1000000, "uatom", 600)
            .await
            .unwrap();

        assert!(!result.id.is_empty());
        assert!(result.tx_hash.is_none());
        assert_eq!(result.amount, 1000000);

        // Release escrow
        let release_result = backend.release_escrow(&result.id, "cosmos1solver...").await;
        assert!(release_result.is_ok());

        // Can't release again
        let double_release = backend.release_escrow(&result.id, "cosmos1solver...").await;
        assert!(double_release.is_err());
    }

    #[tokio::test]
    async fn test_simulated_backend_mode() {
        let backend = SimulatedBackend::new();
        match backend.mode() {
            BackendMode::Simulated => (),
            _ => panic!("Expected Simulated mode"),
        }
    }

    #[tokio::test]
    async fn test_health_check() {
        let backend = SimulatedBackend::new();
        assert!(backend.health_check().await.unwrap());
    }
}
