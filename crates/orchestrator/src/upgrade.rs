//! Upgrade management for zero-downtime backend deployments
//!
//! This module provides graceful upgrade capabilities for the orchestrator
//! and related backend services. It ensures that inflight intents complete
//! successfully during service restarts and upgrades.
//!
//! # Key Components
//!
//! - [`DrainMode`]: State machine for controlling intent acceptance
//! - [`DrainModeManager`]: Controls the drain process
//! - [`InflightTracker`]: Tracks active intents for safe shutdown
//! - [`UpgradeCoordinator`]: Orchestrates the complete upgrade process

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cosmwasm_std::Uint128;
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{sleep, Instant};
use tracing::{error, info, warn};

// ═══════════════════════════════════════════════════════════════════════════
// DRAIN MODE
// ═══════════════════════════════════════════════════════════════════════════

/// Current drain mode state
#[derive(Debug, Clone, PartialEq)]
pub enum DrainMode {
    /// Normal operation - accepting new intents
    Active,

    /// Draining - reject new intents, process existing
    Draining {
        started_at: u64,
        deadline: u64,
        reason: String,
    },

    /// Drained - no inflight intents, safe to upgrade
    Drained { completed_at: u64 },

    /// Upgrading - system is being upgraded
    Upgrading { version: String, started_at: u64 },
}

impl DrainMode {
    pub fn is_accepting_new_intents(&self) -> bool {
        matches!(self, DrainMode::Active)
    }

    pub fn is_safe_to_shutdown(&self) -> bool {
        matches!(self, DrainMode::Drained { .. })
    }
}

/// Manages the drain mode state for graceful shutdown
pub struct DrainModeManager {
    /// Current mode
    mode: Arc<RwLock<DrainMode>>,

    /// Reference to inflight tracker
    inflight_tracker: Arc<InflightTracker>,

    /// Broadcast channel for mode change notifications
    mode_tx: broadcast::Sender<DrainMode>,
}

impl DrainModeManager {
    pub fn new(inflight_tracker: Arc<InflightTracker>) -> Self {
        let (mode_tx, _) = broadcast::channel(16);
        Self {
            mode: Arc::new(RwLock::new(DrainMode::Active)),
            inflight_tracker,
            mode_tx,
        }
    }

    /// Get the current drain mode
    pub async fn current_mode(&self) -> DrainMode {
        self.mode.read().await.clone()
    }

    /// Check if the system is accepting new intents
    pub async fn is_accepting(&self) -> bool {
        self.mode.read().await.is_accepting_new_intents()
    }

    /// Subscribe to mode changes
    pub fn subscribe(&self) -> broadcast::Receiver<DrainMode> {
        self.mode_tx.subscribe()
    }

    /// Start draining - reject new intents and wait for existing to complete
    pub async fn start_drain(&self, reason: String, deadline_secs: u64) -> Result<(), DrainError> {
        let mut mode = self.mode.write().await;

        // Check current state
        match &*mode {
            DrainMode::Active => {
                let now = current_timestamp();
                let new_mode = DrainMode::Draining {
                    started_at: now,
                    deadline: now + deadline_secs,
                    reason: reason.clone(),
                };

                info!(
                    reason = %reason,
                    deadline_secs = deadline_secs,
                    inflight_count = self.inflight_tracker.count(),
                    "Starting drain mode"
                );

                *mode = new_mode.clone();
                let _ = self.mode_tx.send(new_mode);
                Ok(())
            }
            DrainMode::Draining { .. } => {
                warn!("Already in drain mode");
                Err(DrainError::AlreadyDraining)
            }
            DrainMode::Drained { .. } => {
                info!("System already drained");
                Ok(())
            }
            DrainMode::Upgrading { .. } => Err(DrainError::UpgradeInProgress),
        }
    }

    /// Wait for all inflight intents to complete
    pub async fn wait_for_drain(&self, timeout: Duration) -> Result<DrainResult, DrainError> {
        let start = Instant::now();
        let check_interval = Duration::from_secs(1);

        loop {
            let count = self.inflight_tracker.count();

            if count == 0 {
                // All inflight intents completed
                let mut mode = self.mode.write().await;
                let new_mode = DrainMode::Drained {
                    completed_at: current_timestamp(),
                };
                *mode = new_mode.clone();
                let _ = self.mode_tx.send(new_mode);

                info!(
                    elapsed_secs = start.elapsed().as_secs(),
                    "Drain completed - all inflight intents finished"
                );

                return Ok(DrainResult::Completed {
                    elapsed: start.elapsed(),
                    completed_count: self.inflight_tracker.completed_count(),
                });
            }

            if start.elapsed() >= timeout {
                let inflight = self.inflight_tracker.get_all_inflight();
                warn!(
                    remaining_count = count,
                    timeout_secs = timeout.as_secs(),
                    "Drain timeout - some intents still inflight"
                );

                return Ok(DrainResult::TimedOut {
                    elapsed: start.elapsed(),
                    remaining_intents: inflight,
                });
            }

            // Log progress periodically
            if start.elapsed().as_secs() % 10 == 0 && start.elapsed().as_secs() > 0 {
                info!(
                    remaining = count,
                    elapsed_secs = start.elapsed().as_secs(),
                    "Drain in progress..."
                );
            }

            sleep(check_interval).await;
        }
    }

    /// Force mark as drained (use with caution - may lose inflight intents)
    pub async fn force_drain(&self) -> Result<(), DrainError> {
        let count = self.inflight_tracker.count();
        if count > 0 {
            warn!(
                inflight_count = count,
                "Force draining with inflight intents!"
            );
        }

        let mut mode = self.mode.write().await;
        let new_mode = DrainMode::Drained {
            completed_at: current_timestamp(),
        };
        *mode = new_mode.clone();
        let _ = self.mode_tx.send(new_mode);

        Ok(())
    }

    /// Resume normal operation after upgrade
    pub async fn resume(&self) -> Result<(), DrainError> {
        let mut mode = self.mode.write().await;

        match &*mode {
            DrainMode::Drained { .. } | DrainMode::Upgrading { .. } => {
                info!("Resuming normal operation");
                let new_mode = DrainMode::Active;
                *mode = new_mode.clone();
                let _ = self.mode_tx.send(new_mode);
                Ok(())
            }
            DrainMode::Active => {
                info!("Already in active mode");
                Ok(())
            }
            DrainMode::Draining { .. } => Err(DrainError::DrainInProgress),
        }
    }

    /// Cancel drain and resume accepting intents
    pub async fn cancel_drain(&self) -> Result<(), DrainError> {
        let mut mode = self.mode.write().await;

        match &*mode {
            DrainMode::Draining { .. } => {
                info!("Cancelling drain, resuming normal operation");
                let new_mode = DrainMode::Active;
                *mode = new_mode.clone();
                let _ = self.mode_tx.send(new_mode);
                Ok(())
            }
            DrainMode::Active => {
                info!("Not in drain mode");
                Ok(())
            }
            _ => Err(DrainError::InvalidState),
        }
    }

    /// Get drain status for monitoring
    pub async fn get_status(&self) -> DrainStatus {
        let mode = self.mode.read().await.clone();
        let inflight_count = self.inflight_tracker.count();
        let oldest_inflight = self.inflight_tracker.oldest_inflight_age();

        DrainStatus {
            mode,
            inflight_count,
            oldest_inflight_age_secs: oldest_inflight,
            completed_since_drain: self.inflight_tracker.completed_count(),
        }
    }
}

/// Result of a drain operation
#[derive(Debug)]
pub enum DrainResult {
    /// All inflight intents completed successfully
    Completed {
        elapsed: Duration,
        completed_count: u64,
    },

    /// Drain timed out with some intents still inflight
    TimedOut {
        elapsed: Duration,
        remaining_intents: Vec<InflightIntent>,
    },
}

/// Status of the drain process
#[derive(Debug, Clone)]
pub struct DrainStatus {
    pub mode: DrainMode,
    pub inflight_count: u64,
    pub oldest_inflight_age_secs: Option<u64>,
    pub completed_since_drain: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// INFLIGHT TRACKER
// ═══════════════════════════════════════════════════════════════════════════

/// Tracks intents that are currently being processed
#[derive(Debug, Clone)]
pub struct InflightIntent {
    pub id: String,
    pub intent_id: String,
    pub created_at: u64,
    pub phase: InflightPhase,
    pub user_funds_locked: bool,
    pub solver_funds_locked: bool,
    pub ibc_in_flight: bool,
    pub user_amount: Uint128,
    pub solver_id: Option<String>,
}

/// Phase of an inflight intent
#[derive(Debug, Clone, PartialEq)]
pub enum InflightPhase {
    Validating,
    Matching,
    QuotingWithSolvers,
    SelectingRoute,
    LockingUserFunds,
    LockingSolverBond,
    ExecutingIbc,
    WaitingForAck,
    Completing,
}

impl InflightPhase {
    /// Check if this phase has user funds at risk
    pub fn has_locked_funds(&self) -> bool {
        matches!(
            self,
            InflightPhase::LockingUserFunds
                | InflightPhase::LockingSolverBond
                | InflightPhase::ExecutingIbc
                | InflightPhase::WaitingForAck
                | InflightPhase::Completing
        )
    }
}

/// Tracks all inflight intents for graceful shutdown
pub struct InflightTracker {
    /// Active intents by settlement ID
    active: RwLock<HashMap<String, InflightIntent>>,

    /// Counter for quick drain check
    count: AtomicU64,

    /// Total completed since tracker creation
    completed_count: AtomicU64,

    /// Whether we're in drain mode (reject new registrations)
    draining: AtomicBool,
}

impl InflightTracker {
    pub fn new() -> Self {
        Self {
            active: RwLock::new(HashMap::new()),
            count: AtomicU64::new(0),
            completed_count: AtomicU64::new(0),
            draining: AtomicBool::new(false),
        }
    }

    /// Set drain mode - new registrations will be rejected
    pub fn set_draining(&self, draining: bool) {
        self.draining.store(draining, Ordering::SeqCst);
    }

    /// Check if in drain mode
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Register a new inflight intent
    pub async fn register(
        &self,
        settlement_id: &str,
        intent_id: &str,
        user_amount: Uint128,
    ) -> Result<(), InflightError> {
        // Reject if draining
        if self.draining.load(Ordering::SeqCst) {
            return Err(InflightError::DrainModeActive);
        }

        let mut active = self.active.write().await;

        if active.contains_key(settlement_id) {
            return Err(InflightError::AlreadyTracked {
                id: settlement_id.to_string(),
            });
        }

        let intent = InflightIntent {
            id: settlement_id.to_string(),
            intent_id: intent_id.to_string(),
            created_at: current_timestamp(),
            phase: InflightPhase::Validating,
            user_funds_locked: false,
            solver_funds_locked: false,
            ibc_in_flight: false,
            user_amount,
            solver_id: None,
        };

        active.insert(settlement_id.to_string(), intent);
        self.count.fetch_add(1, Ordering::SeqCst);

        Ok(())
    }

    /// Update the phase of an inflight intent
    pub async fn update_phase(
        &self,
        settlement_id: &str,
        phase: InflightPhase,
    ) -> Result<(), InflightError> {
        let mut active = self.active.write().await;

        match active.get_mut(settlement_id) {
            Some(intent) => {
                intent.phase = phase;
                Ok(())
            }
            None => Err(InflightError::NotFound {
                id: settlement_id.to_string(),
            }),
        }
    }

    /// Mark user funds as locked
    pub async fn mark_user_locked(&self, settlement_id: &str) -> Result<(), InflightError> {
        let mut active = self.active.write().await;

        match active.get_mut(settlement_id) {
            Some(intent) => {
                intent.user_funds_locked = true;
                intent.phase = InflightPhase::LockingUserFunds;
                Ok(())
            }
            None => Err(InflightError::NotFound {
                id: settlement_id.to_string(),
            }),
        }
    }

    /// Mark solver funds as locked
    pub async fn mark_solver_locked(
        &self,
        settlement_id: &str,
        solver_id: &str,
    ) -> Result<(), InflightError> {
        let mut active = self.active.write().await;

        match active.get_mut(settlement_id) {
            Some(intent) => {
                intent.solver_funds_locked = true;
                intent.solver_id = Some(solver_id.to_string());
                intent.phase = InflightPhase::LockingSolverBond;
                Ok(())
            }
            None => Err(InflightError::NotFound {
                id: settlement_id.to_string(),
            }),
        }
    }

    /// Mark IBC transfer as in-flight
    pub async fn mark_ibc_inflight(&self, settlement_id: &str) -> Result<(), InflightError> {
        let mut active = self.active.write().await;

        match active.get_mut(settlement_id) {
            Some(intent) => {
                intent.ibc_in_flight = true;
                intent.phase = InflightPhase::ExecutingIbc;
                Ok(())
            }
            None => Err(InflightError::NotFound {
                id: settlement_id.to_string(),
            }),
        }
    }

    /// Mark intent as completed and remove from tracking
    pub async fn complete(&self, settlement_id: &str) -> Result<InflightIntent, InflightError> {
        let mut active = self.active.write().await;

        match active.remove(settlement_id) {
            Some(intent) => {
                self.count.fetch_sub(1, Ordering::SeqCst);
                self.completed_count.fetch_add(1, Ordering::SeqCst);
                Ok(intent)
            }
            None => Err(InflightError::NotFound {
                id: settlement_id.to_string(),
            }),
        }
    }

    /// Get count of inflight intents
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::SeqCst)
    }

    /// Get total completed count
    pub fn completed_count(&self) -> u64 {
        self.completed_count.load(Ordering::SeqCst)
    }

    /// Get all inflight intents
    pub fn get_all_inflight(&self) -> Vec<InflightIntent> {
        // Use try_read to avoid blocking - if locked, return empty
        match self.active.try_read() {
            Ok(active) => active.values().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Get inflight intents with locked funds (most critical for shutdown)
    pub async fn get_with_locked_funds(&self) -> Vec<InflightIntent> {
        let active = self.active.read().await;
        active
            .values()
            .filter(|i| i.user_funds_locked || i.solver_funds_locked)
            .cloned()
            .collect()
    }

    /// Get the age of the oldest inflight intent
    pub fn oldest_inflight_age(&self) -> Option<u64> {
        match self.active.try_read() {
            Ok(active) => {
                let now = current_timestamp();
                active
                    .values()
                    .map(|i| now.saturating_sub(i.created_at))
                    .max()
            }
            Err(_) => None,
        }
    }

    /// Get a specific inflight intent
    pub async fn get(&self, settlement_id: &str) -> Option<InflightIntent> {
        let active = self.active.read().await;
        active.get(settlement_id).cloned()
    }
}

impl Default for InflightTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// GRACEFUL SHUTDOWN
// ═══════════════════════════════════════════════════════════════════════════

/// Handles graceful shutdown with intent protection
pub struct GracefulShutdown {
    drain_manager: Arc<DrainModeManager>,
    shutdown_tx: broadcast::Sender<()>,
    max_drain_time: Duration,
}

impl GracefulShutdown {
    pub fn new(drain_manager: Arc<DrainModeManager>, max_drain_time: Duration) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            drain_manager,
            shutdown_tx,
            max_drain_time,
        }
    }

    /// Subscribe to shutdown signal
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Initiate graceful shutdown
    pub async fn shutdown(&self, reason: &str) -> Result<ShutdownResult, DrainError> {
        info!(reason = %reason, "Initiating graceful shutdown");

        // Start drain
        self.drain_manager
            .start_drain(reason.to_string(), self.max_drain_time.as_secs())
            .await?;

        // Notify subscribers about impending shutdown
        let _ = self.shutdown_tx.send(());

        // Wait for drain with our max time
        let result = self
            .drain_manager
            .wait_for_drain(self.max_drain_time)
            .await?;

        let shutdown_result = match result {
            DrainResult::Completed {
                elapsed,
                completed_count,
            } => {
                info!(
                    elapsed_secs = elapsed.as_secs(),
                    completed = completed_count,
                    "Graceful shutdown complete"
                );
                ShutdownResult::Clean {
                    elapsed,
                    completed_count,
                }
            }
            DrainResult::TimedOut {
                elapsed,
                remaining_intents,
            } => {
                let critical = remaining_intents
                    .iter()
                    .filter(|i| i.phase.has_locked_funds())
                    .count();

                if critical > 0 {
                    error!(
                        remaining = remaining_intents.len(),
                        critical = critical,
                        "Shutdown with critical inflight intents!"
                    );
                } else {
                    warn!(
                        remaining = remaining_intents.len(),
                        "Shutdown with inflight intents (no locked funds)"
                    );
                }

                ShutdownResult::Forced {
                    elapsed,
                    remaining_count: remaining_intents.len() as u64,
                    critical_count: critical as u64,
                }
            }
        };

        Ok(shutdown_result)
    }
}

/// Result of a shutdown operation
#[derive(Debug)]
pub enum ShutdownResult {
    /// Clean shutdown - all intents completed
    Clean {
        elapsed: Duration,
        completed_count: u64,
    },

    /// Forced shutdown with remaining intents
    Forced {
        elapsed: Duration,
        remaining_count: u64,
        critical_count: u64, // Intents with locked funds
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// ERROR TYPES
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Error)]
pub enum DrainError {
    #[error("Already in drain mode")]
    AlreadyDraining,

    #[error("Drain in progress")]
    DrainInProgress,

    #[error("Upgrade in progress")]
    UpgradeInProgress,

    #[error("Invalid state for this operation")]
    InvalidState,
}

#[derive(Debug, Error)]
pub enum InflightError {
    #[error("Drain mode active - not accepting new intents")]
    DrainModeActive,

    #[error("Intent already tracked: {id}")]
    AlreadyTracked { id: String },

    #[error("Intent not found: {id}")]
    NotFound { id: String },
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inflight_tracker_register_complete() {
        let tracker = InflightTracker::new();

        // Register
        tracker
            .register("settlement-1", "intent-1", Uint128::new(1000000))
            .await
            .unwrap();

        assert_eq!(tracker.count(), 1);

        // Complete
        let intent = tracker.complete("settlement-1").await.unwrap();
        assert_eq!(intent.intent_id, "intent-1");
        assert_eq!(tracker.count(), 0);
        assert_eq!(tracker.completed_count(), 1);
    }

    #[tokio::test]
    async fn test_inflight_tracker_draining() {
        let tracker = InflightTracker::new();

        // Register normally
        tracker
            .register("settlement-1", "intent-1", Uint128::new(1000000))
            .await
            .unwrap();

        // Set draining
        tracker.set_draining(true);

        // Try to register - should fail
        let result = tracker
            .register("settlement-2", "intent-2", Uint128::new(1000000))
            .await;

        assert!(matches!(result, Err(InflightError::DrainModeActive)));
    }

    #[tokio::test]
    async fn test_drain_mode_manager_lifecycle() {
        let tracker = Arc::new(InflightTracker::new());
        let manager = DrainModeManager::new(tracker.clone());

        // Initially active
        assert!(manager.is_accepting().await);

        // Start drain
        manager
            .start_drain("test upgrade".to_string(), 60)
            .await
            .unwrap();

        // Should not be accepting
        assert!(!manager.is_accepting().await);

        // Cancel drain
        manager.cancel_drain().await.unwrap();

        // Should be accepting again
        assert!(manager.is_accepting().await);
    }

    #[tokio::test]
    async fn test_drain_completes_when_empty() {
        let tracker = Arc::new(InflightTracker::new());
        let manager = DrainModeManager::new(tracker.clone());

        // Start drain with no inflight
        manager
            .start_drain("test".to_string(), 60)
            .await
            .unwrap();

        // Should complete immediately
        let result = manager.wait_for_drain(Duration::from_secs(5)).await.unwrap();

        assert!(matches!(result, DrainResult::Completed { .. }));
    }

    #[tokio::test]
    async fn test_update_phases() {
        let tracker = InflightTracker::new();

        tracker
            .register("s-1", "i-1", Uint128::new(1000000))
            .await
            .unwrap();

        // Initial phase
        let intent = tracker.get("s-1").await.unwrap();
        assert_eq!(intent.phase, InflightPhase::Validating);

        // Update phase
        tracker
            .update_phase("s-1", InflightPhase::Matching)
            .await
            .unwrap();

        let intent = tracker.get("s-1").await.unwrap();
        assert_eq!(intent.phase, InflightPhase::Matching);

        // Mark locked
        tracker.mark_user_locked("s-1").await.unwrap();

        let intent = tracker.get("s-1").await.unwrap();
        assert!(intent.user_funds_locked);
        assert!(intent.phase.has_locked_funds());
    }

    #[tokio::test]
    async fn test_get_with_locked_funds() {
        let tracker = InflightTracker::new();

        // Register two intents
        tracker
            .register("s-1", "i-1", Uint128::new(1000000))
            .await
            .unwrap();
        tracker
            .register("s-2", "i-2", Uint128::new(2000000))
            .await
            .unwrap();

        // Lock funds for one
        tracker.mark_user_locked("s-1").await.unwrap();

        // Get with locked funds
        let locked = tracker.get_with_locked_funds().await;
        assert_eq!(locked.len(), 1);
        assert_eq!(locked[0].id, "s-1");
    }
}
