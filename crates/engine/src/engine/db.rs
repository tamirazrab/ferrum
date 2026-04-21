use super::engine::Engine;
use crate::types::{
    db::{DatabaseRequests, DbOpenOrder, DbTrade, DbUserBalance},
    engine::{Fill, Order, OrderSide, OrderStatus},
};
use async_trait::async_trait;
use redis::{RedisManager, RedisQueues};
use rust_decimal::Decimal;

#[async_trait]
pub trait DbUpdates {
    async fn update_db_orders(
        &self,
        order: Order,
        taker_market: &str,
        executed_quantity: Decimal,
        fills: &[Fill],
        redis_conn: &RedisManager,
    );
    async fn create_db_trades(
        &self,
        user_id: String,
        market: String,
        fills: &[Fill],
        redis_conn: &RedisManager,
    );
    async fn persist_order_to_db(&self, order: &Order, market: &str, redis_conn: &RedisManager);
    async fn delete_order_from_db(&self, order_id: &str, redis_conn: &RedisManager);
    async fn persist_balance_to_db(
        &self,
        user_id: &str,
        asset: &str,
        available: Decimal,
        locked: Decimal,
        redis_conn: &RedisManager,
    );
}

async fn push_db_request(redis_conn: &RedisManager, request: &DatabaseRequests) {
    match serde_json::to_string(request) {
        Ok(data) => {
            if let Err(e) = redis_conn
                .push(&RedisQueues::Database.to_string(), data)
                .await
            {
                tracing::error!(%e, "Failed to push to database queue");
            }
        }
        Err(e) => tracing::error!(%e, "Failed to serialize db request"),
    }
}

#[async_trait]
impl DbUpdates for Engine {
    async fn update_db_orders(
        &self,
        order: Order,
        taker_market: &str,
        executed_quantity: Decimal,
        fills: &[Fill],
        redis_conn: &RedisManager,
    ) {
        let market_on_book = self
            .orderbooks
            .iter()
            .find(|ob| {
                ob.get_open_order(&order.user_id, &order.order_id)
                    .is_some()
            })
            .map(|ob| ob.ticker());

        let taker_market_str = market_on_book
            .unwrap_or_else(|| taker_market.to_string());

        if executed_quantity < order.quantity {
            let status = if executed_quantity > Decimal::ZERO {
                "PartiallyFilled"
            } else {
                "Pending"
            };
            let side_str = match order.side {
                OrderSide::Buy => "Buy",
                OrderSide::Sell => "Sell",
            };
            let db_order = DbOpenOrder {
                order_id: order.order_id.clone(),
                user_id: order.user_id.clone(),
                market: taker_market_str,
                side: side_str.to_string(),
                price: order.price,
                quantity: order.quantity,
                filled_quantity: executed_quantity,
                order_status: status.to_string(),
                timestamp: order.timestamp,
            };
            push_db_request(redis_conn, &DatabaseRequests::UpsertOrder(db_order)).await;
        } else {
            push_db_request(
                redis_conn,
                &DatabaseRequests::DeleteOrder {
                    order_id: order.order_id.clone(),
                },
            )
            .await;
        }

        for fill in fills {
            let maker_filled = self
                .orderbooks
                .iter()
                .flat_map(|ob| {
                    ob.get_open_order(&fill.other_user_id, &fill.order_id)
                        .map(|o| (ob.ticker(), o.clone()))
                })
                .next();

            if let Some((mkt, maker_order)) = maker_filled {
                let status = if maker_order.filled_quantity >= maker_order.quantity {
                    push_db_request(
                        redis_conn,
                        &DatabaseRequests::DeleteOrder {
                            order_id: fill.order_id.clone(),
                        },
                    )
                    .await;
                    continue;
                } else {
                    "PartiallyFilled"
                };
                let side_str = match maker_order.side {
                    OrderSide::Buy => "Buy",
                    OrderSide::Sell => "Sell",
                };
                let db_order = DbOpenOrder {
                    order_id: maker_order.order_id.clone(),
                    user_id: maker_order.user_id.clone(),
                    market: mkt,
                    side: side_str.to_string(),
                    price: maker_order.price,
                    quantity: maker_order.quantity,
                    filled_quantity: maker_order.filled_quantity,
                    order_status: status.to_string(),
                    timestamp: maker_order.timestamp,
                };
                push_db_request(redis_conn, &DatabaseRequests::UpsertOrder(db_order)).await;
            } else {
                push_db_request(
                    redis_conn,
                    &DatabaseRequests::DeleteOrder {
                        order_id: fill.order_id.clone(),
                    },
                )
                .await;
            }
        }
    }

    async fn create_db_trades(
        &self,
        user_id: String,
        market: String,
        fills: &[Fill],
        redis_conn: &RedisManager,
    ) {
        for fill in fills.iter() {
            let db_trade = DbTrade {
                trade_id: fill.trade_id,
                price: fill.price,
                quantity: fill.quantity,
                market: market.clone(),
                user_id: user_id.clone(),
                other_user_id: fill.other_user_id.clone(),
                order_id: fill.order_id.clone(),
                timestamp: chrono::Utc::now().timestamp_millis(),
            };

            push_db_request(redis_conn, &DatabaseRequests::InsertTrade(db_trade)).await;
        }
    }

    async fn persist_order_to_db(&self, order: &Order, market: &str, redis_conn: &RedisManager) {
        let side_str = match order.side {
            OrderSide::Buy => "Buy",
            OrderSide::Sell => "Sell",
        };
        let status_str = match order.order_status {
            OrderStatus::Pending => "Pending",
            OrderStatus::Filled => "Filled",
            OrderStatus::PartiallyFilled => "PartiallyFilled",
            OrderStatus::Cancelled => "Cancelled",
        };
        let db_order = DbOpenOrder {
            order_id: order.order_id.clone(),
            user_id: order.user_id.clone(),
            market: market.to_string(),
            side: side_str.to_string(),
            price: order.price,
            quantity: order.quantity,
            filled_quantity: order.filled_quantity,
            order_status: status_str.to_string(),
            timestamp: order.timestamp,
        };
        push_db_request(redis_conn, &DatabaseRequests::UpsertOrder(db_order)).await;
    }

    async fn delete_order_from_db(&self, order_id: &str, redis_conn: &RedisManager) {
        push_db_request(
            redis_conn,
            &DatabaseRequests::DeleteOrder {
                order_id: order_id.to_string(),
            },
        )
        .await;
    }

    async fn persist_balance_to_db(
        &self,
        user_id: &str,
        asset: &str,
        available: Decimal,
        locked: Decimal,
        redis_conn: &RedisManager,
    ) {
        let balance = DbUserBalance {
            user_id: user_id.to_string(),
            asset: asset.to_string(),
            available,
            locked,
        };
        push_db_request(redis_conn, &DatabaseRequests::UpsertBalance(balance)).await;
    }
}
