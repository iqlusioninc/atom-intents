use thiserror::Error;

#[derive(Debug, Error)]
pub enum SolveError {
    #[error("no viable route found")]
    NoViableRoute,

    #[error("intent expired")]
    IntentExpired,

    #[error("insufficient liquidity: needed {needed}, available {available}")]
    InsufficientLiquidity { needed: String, available: String },

    #[error("price exceeds limit: limit {limit}, offered {offered}")]
    PriceExceedsLimit { limit: String, offered: String },

    #[error("venue excluded: {venue}")]
    VenueExcluded { venue: String },

    #[error("solver capacity exceeded")]
    CapacityExceeded,

    #[error("invalid intent: {reason}")]
    InvalidIntent { reason: String },

    #[error("dex query failed: {0}")]
    DexQueryFailed(String),

    #[error("cex query failed: {0}")]
    CexQueryFailed(String),

    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Error)]
pub enum ReputationError {
    #[error("network error: {0}")]
    Network(String),

    #[error("query failed: {0}")]
    QueryFailed(String),

    #[error("parse error: {0}")]
    ParseError(String),
}
