use rust_decimal::Decimal;
use thiserror::Error;

/// Maximum safe amount for decimal arithmetic (10^28)
/// rust_decimal max is ~7.9 × 10^28, we use 10^28 for safety margin
pub const MAX_SAFE_AMOUNT: u128 = 10_000_000_000_000_000_000_000_000_000;

/// Maximum allowed deviation from oracle for sanity check (10%)
pub const MAX_ORACLE_DEVIATION: &str = "0.10";

/// SECURITY FIX (1.1): Maximum allowed oracle confidence/uncertainty (5%)
/// If oracle reports higher uncertainty, we should reject or widen tolerance
pub const MAX_ORACLE_CONFIDENCE: &str = "0.05";

/// SECURITY FIX (1.6): Maximum number of solver quotes per auction
/// Prevents DoS via excessive quotes
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

    #[error(
        "buy order price exceeds limit: oracle_price={oracle_price}, limit_price={limit_price}"
    )]
    PriceExceedsLimit {
        oracle_price: Decimal,
        limit_price: Decimal,
    },

    #[error(
        "sell order price below limit: oracle_price={oracle_price}, limit_price={limit_price}"
    )]
    PriceBelowLimit {
        oracle_price: Decimal,
        limit_price: Decimal,
    },

    #[error("amount too large: {amount} exceeds maximum safe amount {max}")]
    AmountTooLarge { amount: u128, max: u128 },

    #[error("execution price {execution_price} deviates {deviation}% from oracle {oracle_price}, max allowed {max_deviation}%")]
    PriceDeviationTooLarge {
        execution_price: Decimal,
        oracle_price: Decimal,
        deviation: Decimal,
        max_deviation: Decimal,
    },

    /// SECURITY FIX (1.1): Oracle price uncertainty too high
    #[error("oracle price uncertainty {confidence} exceeds maximum allowed {max_confidence}")]
    OraclePriceUncertain {
        confidence: Decimal,
        max_confidence: Decimal,
    },

    /// SECURITY FIX (1.4): Intent nonce already used (replay attack prevented)
    #[error("intent nonce {nonce} already used for user {user}")]
    NonceAlreadyUsed { user: String, nonce: u64 },

    /// SECURITY FIX (1.6): Too many solver quotes in auction
    #[error("too many solver quotes: {count} exceeds maximum {max}")]
    TooManyQuotes { count: usize, max: usize },

    /// SECURITY FIX (1.5): Intent has expired
    #[error("intent {intent_id} has expired at {expires_at}, current time is {current_time}")]
    IntentExpired {
        intent_id: String,
        expires_at: u64,
        current_time: u64,
    },
}
