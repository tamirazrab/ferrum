use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DatabaseRequests {
    InsertTrade(DbTrade),
    UpsertOrder(DbOpenOrder),
    DeleteOrder { order_id: String },
    UpsertBalance(DbUserBalance),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTrade {
    pub trade_id: i64,
    pub market: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub user_id: String,
    pub other_user_id: String,
    pub order_id: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbOpenOrder {
    pub order_id: String,
    pub user_id: String,
    pub market: String,
    pub side: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub order_status: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbUserBalance {
    pub user_id: String,
    pub asset: String,
    pub available: Decimal,
    pub locked: Decimal,
}
