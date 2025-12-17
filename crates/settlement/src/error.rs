use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettlementError {
    #[error("escrow lock failed: {0}")]
    EscrowLockFailed(String),

    #[error("solver vault lock failed: {0}")]
    SolverVaultLockFailed(String),

    #[error("IBC transfer failed: {0}")]
    IbcTransferFailed(String),

    #[error("IBC timeout")]
    IbcTimeout,

    #[error("invalid settlement state: {0}")]
    InvalidState(String),

    #[error("settlement already complete")]
    AlreadyComplete,

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("timeout configuration error: {0}")]
    TimeoutError(String),
}
