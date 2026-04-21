use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderInput {
    pub market: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub side: OrderSide,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

impl CreateOrderInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.price <= Decimal::ZERO {
            return Err("price must be positive");
        }
        if self.quantity <= Decimal::ZERO {
            return Err("quantity must be positive");
        }
        if self.market.is_empty() {
            return Err("market is required");
        }
        if self.user_id.is_empty() {
            return Err("user_id is required");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOpenOrderInput {
    pub user_id: String,
    pub order_id: String,
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderInput {
    pub order_id: String,
    pub user_id: String,
    pub price: Decimal,
    pub side: OrderSide,
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

impl CancelOrderInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.order_id.is_empty() {
            return Err("order_id is required");
        }
        if self.user_id.is_empty() {
            return Err("user_id is required");
        }
        if self.price <= Decimal::ZERO {
            return Err("price must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOpenOrdersInput {
    pub user_id: String,
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelAllOrdersInput {
    pub user_id: String,
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDepthInput {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubsub_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTradesInput {
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetKlinesInput {
    pub symbol: String,
    pub interval: String,
    pub start_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderRequests {
    CreateOrder(CreateOrderInput),
    GetOpenOrder(GetOpenOrderInput),
    CancelOrder(CancelOrderInput),
    GetOpenOrders(GetOpenOrdersInput),
    CancelAllOrders(CancelAllOrdersInput),
    GetDepth(GetDepthInput),
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
