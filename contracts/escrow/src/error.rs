use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Escrow not found: {id}")]
    EscrowNotFound { id: String },

    #[error("Escrow already exists: {id}")]
    EscrowAlreadyExists { id: String },

    #[error("Escrow expired: {id}")]
    EscrowExpired { id: String },

    #[error("Escrow not expired: {id}")]
    EscrowNotExpired { id: String },

    #[error("Invalid funds: expected {expected}, got {got}")]
    InvalidFunds { expected: String, got: String },

    #[error("Insufficient funds")]
    InsufficientFunds {},
}
