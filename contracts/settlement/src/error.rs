use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Intent not found: {id}")]
    IntentNotFound { id: String },

    #[error("Settlement not found: {id}")]
    SettlementNotFound { id: String },

    #[error("Settlement already exists: {id}")]
    SettlementAlreadyExists { id: String },

    #[error("Solver not registered: {id}")]
    SolverNotRegistered { id: String },

    #[error("Insufficient bond: required {required}, provided {provided}")]
    InsufficientBond { required: String, provided: String },

    #[error("Invalid state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error("Settlement expired")]
    SettlementExpired {},

    #[error("Inflight settlements exist: {count} settlements must complete before migration")]
    InflightSettlementsExist { count: u64 },

    #[error("Migration failed: {reason}")]
    MigrationFailed { reason: String },

    #[error("Insufficient funds: required {required}, provided {provided}")]
    InsufficientFunds { required: String, provided: String },
}
