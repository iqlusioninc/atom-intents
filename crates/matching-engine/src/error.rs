use rust_decimal::Decimal;
use thiserror::Error;

/// Maximum safe amount for decimal arithmetic (10^28)
/// rust_decimal max is ~7.9 × 10^28, we use 10^28 for safety margin
pub const MAX_SAFE_AMOUNT: u128 = 10_000_000_000_000_000_000_000_000_000;

/// Maximum allowed deviation from oracle for sanity check (10%)
pub const MAX_ORACLE_DEVIATION: &str = "0.10";

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
}
