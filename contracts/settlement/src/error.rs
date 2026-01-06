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

    #[error("Invalid IBC channel: {channel}")]
    InvalidIbcChannel { channel: String },

    // ═══════════════════════════════════════════════════════════════════════════
    // ON-CHAIN ORDER ERRORS
    // ═══════════════════════════════════════════════════════════════════════════

    #[error("No funds sent with message")]
    NoFundsSent {},

    #[error("Multiple denominations not supported")]
    MultipleDenominations {},

    #[error("Invalid timeout: must be between {min} and {max} seconds, got {provided}")]
    InvalidTimeout { min: u64, max: u64, provided: u64 },

    #[error("Order already exists: {id}")]
    OrderAlreadyExists { id: String },

    #[error("Order not found: {id}")]
    OrderNotFound { id: String },

    #[error("Order not open: {id} (status: {status})")]
    OrderNotOpen { id: String, status: String },

    #[error("Order expired: {id}")]
    OrderExpired { id: String },

    #[error("Order not expired: {id} (expires_at: {expires_at}, current: {current_time})")]
    OrderNotExpired {
        id: String,
        expires_at: u64,
        current_time: u64,
    },
}
