use thiserror::Error;
use rust_decimal::Decimal;

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
}
