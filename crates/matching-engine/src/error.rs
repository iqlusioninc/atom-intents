use thiserror::Error;

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
}
