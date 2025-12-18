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

    #[error("channel error: {0}")]
    ChannelError(#[from] ChannelError),
}

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("channel not found for route {0} -> {1}")]
    ChannelNotFound(String, String),

    #[error("invalid channel: {0}")]
    InvalidChannel(String),

    #[error("channel is closed")]
    ChannelClosed,
}
