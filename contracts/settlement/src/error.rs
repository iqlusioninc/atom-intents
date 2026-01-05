use atom_intents_types::collateral::CollateralError;
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

    // ═══════════════════════════════════════════════════════════════════════════
    // COLLATERAL ERRORS
    // ═══════════════════════════════════════════════════════════════════════════

    #[error("No bond pool found for solver: {solver_id}")]
    NoBondPool { solver_id: String },

    #[error("Insufficient collateral: required {required} after haircut, available {available}")]
    InsufficientCollateral { required: String, available: String },

    #[error("No collateral locked for settlement: {settlement_id}")]
    NoLockedCollateral { settlement_id: String },

    #[error("Collateral error: {0}")]
    Collateral(#[from] CollateralError),

    #[error("Liquidation intent not found: {id}")]
    LiquidationNotFound { id: String },

    #[error("Liquidation intent already exists: {id}")]
    LiquidationAlreadyExists { id: String },

    #[error("Slashed solver cannot bid on liquidation")]
    SlashedSolverCannotBid {},

    #[error("Bid amount below minimum: offered {offered}, minimum {minimum}")]
    BidBelowMinimum { offered: String, minimum: String },

    #[error("Stale pool info for {share_denom}: {age_seconds}s old (max {max_seconds}s)")]
    StalePoolInfo {
        share_denom: String,
        age_seconds: u64,
        max_seconds: u64,
    },

    #[error("No cached pool info for {share_denom}")]
    NoCachedPoolInfo { share_denom: String },
}
