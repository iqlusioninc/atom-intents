use thiserror::Error;
use rust_decimal::Decimal;

/// Maximum acceptable oracle confidence interval (2% = 0.02)
/// If confidence exceeds this, the oracle price is too uncertain for safe matching
pub const MAX_CONFIDENCE_THRESHOLD: &str = "0.02";

/// Maximum number of solver quotes per auction to prevent DoS
pub const MAX_QUOTES_PER_AUCTION: usize = 100;

#[derive(Debug, Error)]
pub enum MatchingError {
    #[error("intent not found: {0}")]
    IntentNotFound(String),

    #[error("invalid price: {0}")]
    InvalidPrice(String),

    #[error("order book empty")]
    EmptyBook,

    #[error("prices do not cross")]
    PricesDoNotCross,

    #[error("partial fill not allowed")]
    PartialFillNotAllowed,

    #[error("buy order price exceeds limit: oracle_price={oracle_price}, limit_price={limit_price}")]
    PriceExceedsLimit {
        oracle_price: Decimal,
        limit_price: Decimal,
    },

    #[error("sell order price below limit: oracle_price={oracle_price}, limit_price={limit_price}")]
    PriceBelowLimit {
        oracle_price: Decimal,
        limit_price: Decimal,
    },

    #[error("oracle price too uncertain: confidence={confidence}, threshold={threshold}")]
    OraclePriceUncertain {
        confidence: Decimal,
        threshold: Decimal,
    },

    #[error("intent expired: expires_at={expires_at}, current_time={current_time}")]
    IntentExpired {
        expires_at: u64,
        current_time: u64,
    },

    #[error("too many solver quotes: count={count}, max={max}")]
    TooManyQuotes {
        count: usize,
        max: usize,
    },

    #[error("nonce already used: user={user}, nonce={nonce}")]
    NonceAlreadyUsed {
        user: String,
        nonce: u64,
    },
}
