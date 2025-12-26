//! Execution backend abstraction for demo modes
//!
//! This module provides a unified interface for executing settlements,
//! allowing the demo to switch between simulated and real testnet modes.

pub mod config;
pub mod simulated;
pub mod testnet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::models::{Settlement, SettlementStatus};

/// Backend execution mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendMode {
    /// Pure in-memory simulation (no blockchain)
    Simulated,
    /// Connected to a real testnet
    Testnet {
        chain_id: String,
        settlement_contract: String,
        escrow_contract: String,
    },
    /// Connected to local docker chains
    Localnet {
        settlement_contract: String,
        escrow_contract: String,
    },
}

/// Result of locking funds in escrow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscrowLockResult {
    /// Unique ID for this escrow lock
    pub id: String,
    /// Transaction hash (None for simulated mode)
    pub tx_hash: Option<String>,
    /// Block height where lock was confirmed (None for simulated)
    pub block_height: Option<u64>,
    /// Amount locked
    pub amount: u128,
    /// Denomination
    pub denom: String,
}

/// Result of settlement execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementResult {
    /// Settlement ID
    pub id: String,
    /// Final status
    pub status: SettlementStatus,
    /// Transaction hash for the settlement (testnet only)
    pub tx_hash: Option<String>,
    /// Block height (testnet only)
    pub block_height: Option<u64>,
    /// Explorer URL for the transaction
    pub explorer_url: Option<String>,
}

/// Backend error types
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("chain not configured: {0}")]
    ChainNotConfigured(String),

    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("contract call failed: {0}")]
    ContractCallFailed(String),

    #[error("transaction failed: {0}")]
    TransactionFailed(String),

    #[error("escrow lock failed: {0}")]
    EscrowLockFailed(String),

    #[error("settlement failed: {0}")]
    SettlementFailed(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("configuration error: {0}")]
    ConfigError(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// Settlement event for real-time updates
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendEvent {
    /// Escrow funds were locked
    EscrowLocked {
        settlement_id: String,
        escrow_id: String,
        tx_hash: Option<String>,
        block_height: Option<u64>,
        amount: u128,
        denom: String,
    },
    /// Solver committed their output
    SolverCommitted {
        settlement_id: String,
        solver_id: String,
        tx_hash: Option<String>,
    },
    /// IBC transfer initiated
    IbcTransferStarted {
        settlement_id: String,
        packet_sequence: Option<u64>,
        tx_hash: Option<String>,
    },
    /// IBC transfer completed
    IbcTransferComplete {
        settlement_id: String,
        tx_hash: Option<String>,
    },
    /// Settlement completed successfully
    SettlementComplete {
        settlement_id: String,
        tx_hash: Option<String>,
        output_delivered: u128,
    },
    /// Settlement failed
    SettlementFailed {
        settlement_id: String,
        reason: String,
        recoverable: bool,
    },
    /// Mode changed
    ModeChanged {
        mode: BackendMode,
    },
}

/// Execution backend trait - implemented by simulated and testnet backends
#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    /// Get the current execution mode
    fn mode(&self) -> BackendMode;

    /// Lock user funds in escrow
    async fn lock_escrow(
        &self,
        settlement_id: &str,
        user: &str,
        amount: u128,
        denom: &str,
        timeout_secs: u64,
    ) -> Result<EscrowLockResult, BackendError>;

    /// Release escrow to recipient (on successful settlement)
    async fn release_escrow(
        &self,
        escrow_id: &str,
        recipient: &str,
    ) -> Result<Option<String>, BackendError>;

    /// Refund escrow to original owner (on failed settlement)
    async fn refund_escrow(&self, escrow_id: &str) -> Result<Option<String>, BackendError>;

    /// Execute a full settlement
    async fn execute_settlement(
        &self,
        settlement: &Settlement,
    ) -> Result<SettlementResult, BackendError>;

    /// Query current settlement status
    async fn get_settlement_status(
        &self,
        settlement_id: &str,
    ) -> Result<SettlementStatus, BackendError>;

    /// Get contract addresses (testnet only, returns None for simulated)
    fn contract_addresses(&self) -> Option<ContractAddresses>;

    /// Subscribe to backend events
    fn subscribe(&self) -> broadcast::Receiver<BackendEvent>;

    /// Check if the backend is healthy/connected
    async fn health_check(&self) -> Result<bool, BackendError>;
}

/// Contract addresses for testnet mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAddresses {
    pub settlement: String,
    pub escrow: String,
}

/// Explorer URL builder for different networks
pub fn build_explorer_url(chain_id: &str, tx_hash: &str) -> Option<String> {
    match chain_id {
        "theta-testnet-001" => Some(format!(
            "https://www.mintscan.io/cosmos-testnet/tx/{}",
            tx_hash
        )),
        "osmo-test-5" => Some(format!(
            "https://testnet.mintscan.io/osmosis-testnet/tx/{}",
            tx_hash
        )),
        "pion-1" => Some(format!(
            "https://www.mintscan.io/neutron-testnet/tx/{}",
            tx_hash
        )),
        id if id.starts_with("local") => Some(format!("http://localhost:1317/cosmos/tx/v1beta1/txs/{}", tx_hash)),
        _ => None,
    }
}
