use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// Supported assets on the exchange.
#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub enum Asset {
    Usdc,
    Usdt,
    Btc,
    Eth,
    Sol,
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Asset::Usdc => write!(f, "USDC"),
            Asset::Usdt => write!(f, "USDT"),
            Asset::Btc => write!(f, "BTC"),
            Asset::Eth => write!(f, "ETH"),
            Asset::Sol => write!(f, "SOL"),
        }
    }
}

impl FromStr for Asset {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "USDC" => Ok(Asset::Usdc),
            "USDT" => Ok(Asset::Usdt),
            "BTC" => Ok(Asset::Btc),
            "ETH" => Ok(Asset::Eth),
            "SOL" => Ok(Asset::Sol),
            _ => Err("Unsupported asset"),
        }
    }
}

/// A trading pair consisting of a base and quote asset.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetPair {
    pub base: Asset,
    pub quote: Asset,
}

impl AssetPair {
    pub fn ticker(&self) -> String {
        format!("{}_{}", self.base, self.quote)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Filled,
    PartiallyFilled,
    Cancelled,
}

/// A resting or filled order on the book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub price: Decimal,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub order_id: String,
    pub user_id: String,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub order_status: OrderStatus,
    pub timestamp: i64,
}

/// A single fill generated during matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub price: Decimal,
    pub quantity: Decimal,
    pub trade_id: i64,
    pub other_user_id: String,
    pub order_id: String,
}

/// The result of processing an incoming order through the matching engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessOrderResult {
    pub executed_quantity: Decimal,
    pub fills: Vec<Fill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrder {
    pub market: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub side: OrderSide,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOpenOrder {
    pub user_id: String,
    pub order_id: String,
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrder {
    pub order_id: String,
    pub user_id: String,
    pub price: Decimal,
    pub side: OrderSide,
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOpenOrders {
    pub user_id: String,
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelAllOrders {
    pub user_id: String,
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDepth {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderRequests {
    CreateOrder(CreateOrder),
    GetOpenOrder(GetOpenOrder),
    CancelOrder(CancelOrder),
    GetOpenOrders(GetOpenOrders),
    GetDepth(GetDepth),
    CancelAllOrders(CancelAllOrders),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserInput {
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserRequests {
    CreateUser(CreateUserInput),
}