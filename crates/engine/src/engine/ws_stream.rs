use super::engine::Engine;
use crate::types::{
    engine::{Fill, OrderSide},
    ws_stream::WsResponse,
};
use async_trait::async_trait;
use redis::RedisManager;
use rust_decimal::Decimal;

#[async_trait]
pub trait WsStreamUpdates {
    async fn publish_ws_trades(
        &self,
        market: String,
        user_id: String,
        fills: &[Fill],
        timestamp: i64,
        redis_conn: &RedisManager,
    );

    async fn publish_ws_depth_updates(
        &mut self,
        market: String,
        price: Decimal,
        side: OrderSide,
        fills: &[Fill],
        redis_conn: &RedisManager,
    );
}

#[async_trait]
impl WsStreamUpdates for Engine {
    async fn publish_ws_trades(
        &self,
        market: String,
        user_id: String,
        fills: &[Fill],
        timestamp: i64,
        redis_conn: &RedisManager,
    ) {
        for fill in fills.iter() {
            let stream = format!("trade.{market}");
            let data = serde_json::json!({
                "e": "trade",
                "t": fill.trade_id,
                "m": fill.other_user_id == user_id,
                "p": fill.price,
                "q": fill.quantity,
                "s": market,
                "T": timestamp,
            });

            let ws_response = WsResponse {
                stream: stream.clone(),
                data,
            };

            match serde_json::to_string(&ws_response) {
                Ok(payload) => {
                    if let Err(e) = redis_conn.publish(&stream, payload).await {
                        tracing::error!(%e, "Error publishing trade to redis");
                    }
                }
                Err(e) => tracing::error!(%e, "Failed to serialize ws trade"),
            }
        }
    }

    async fn publish_ws_depth_updates(
        &mut self,
        market: String,
        price: Decimal,
        side: OrderSide,
        fills: &[Fill],
        redis_conn: &RedisManager,
    ) {
        let orderbook = match self
            .orderbooks
            .iter()
            .find(|ob| ob.ticker() == market)
        {
            Some(ob) => ob,
            None => return,
        };

        let (depth_bids, depth_asks) = orderbook.get_depth();

        let (updated_bids, updated_asks) = match side {
            OrderSide::Buy => {
                let asks: Vec<_> = depth_asks
                    .into_iter()
                    .filter(|ask| fills.iter().any(|fill| fill.price == ask.0))
                    .collect();
                let bids: Vec<_> = depth_bids
                    .into_iter()
                    .filter(|bid| bid.0 == price)
                    .collect();
                (bids, asks)
            }
            OrderSide::Sell => {
                let bids: Vec<_> = depth_bids
                    .into_iter()
                    .filter(|bid| fills.iter().any(|fill| fill.price == bid.0))
                    .collect();
                let asks: Vec<_> = depth_asks
                    .into_iter()
                    .filter(|ask| ask.0 == price)
                    .collect();
                (bids, asks)
            }
        };

        let stream = format!("depth.{market}");
        let data = serde_json::json!({
            "e": "depth",
            "s": market,
            "b": updated_bids,
            "a": updated_asks,
        });

        let ws_response = WsResponse {
            stream: stream.clone(),
            data,
        };

        match serde_json::to_string(&ws_response) {
            Ok(payload) => {
                if let Err(e) = redis_conn.publish(&stream, payload).await {
                    tracing::error!(%e, "Error publishing depth to redis");
                }
            }
            Err(e) => tracing::error!(%e, "Failed to serialize ws depth"),
        }
    }
}
