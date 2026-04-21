use std::fmt;

/// Errors produced by the exchange engine.
#[derive(Debug)]
pub enum EngineError {
    UnsupportedAsset(String),
    OrderBookNotFound(String),
    InsufficientFunds,
    UserNotFound(String),
    MutexPoisoned,
    NoBalanceForAsset(String),
    OrderNotFound,
    InvalidMarketFormat(String),
    SerializationError(String),
    RedisError(String),
    InvalidPrice,
    InvalidQuantity,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::UnsupportedAsset(s) => write!(f, "unsupported asset: {s}"),
            EngineError::OrderBookNotFound(m) => write!(f, "no orderbook for market: {m}"),
            EngineError::InsufficientFunds => write!(f, "insufficient funds"),
            EngineError::UserNotFound(u) => write!(f, "user not found: {u}"),
            EngineError::MutexPoisoned => write!(f, "mutex poisoned"),
            EngineError::NoBalanceForAsset(a) => write!(f, "no balance for asset: {a}"),
            EngineError::OrderNotFound => write!(f, "order not found"),
            EngineError::InvalidMarketFormat(m) => write!(f, "invalid market format: {m}"),
            EngineError::SerializationError(e) => write!(f, "serialization error: {e}"),
            EngineError::RedisError(e) => write!(f, "redis error: {e}"),
            EngineError::InvalidPrice => write!(f, "price must be positive"),
            EngineError::InvalidQuantity => write!(f, "quantity must be positive"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<&'static str> for EngineError {
    fn from(s: &'static str) -> Self {
        EngineError::UnsupportedAsset(s.to_string())
    }
}

impl From<serde_json::Error> for EngineError {
    fn from(e: serde_json::Error) -> Self {
        EngineError::SerializationError(e.to_string())
    }
}
