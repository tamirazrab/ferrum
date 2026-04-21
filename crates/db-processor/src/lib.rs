pub mod query;
pub mod types;

use fred::prelude::RedisValue;
use query::{delete_open_order, insert_trade, upsert_open_order, upsert_user_balance};
use serde_json::from_str;
use sqlx::{Pool, Postgres};
use types::DatabaseRequests;

pub async fn handle_db_updates(data: Vec<RedisValue>, pg_pool: &Pool<Postgres>) {
    let db_data = match data.first() {
        Some(RedisValue::String(s)) => s.to_string(),
        Some(RedisValue::Bytes(b)) => {
            match String::from_utf8(b.to_vec()) {
                Ok(s) => s,
                Err(_) => {
                    tracing::error!("failed to decode bytes from redis");
                    return;
                }
            }
        }
        _ => {
            tracing::error!("unexpected redis value type in database queue");
            return;
        }
    };

    match from_str::<DatabaseRequests>(&db_data) {
        Ok(DatabaseRequests::InsertTrade(trade)) => {
            if let Err(e) = insert_trade(pg_pool, trade).await {
                tracing::error!(%e, "failed to insert trade");
            }
        }
        Ok(DatabaseRequests::UpsertOrder(order)) => {
            if let Err(e) = upsert_open_order(
                pg_pool,
                &order.order_id,
                &order.user_id,
                &order.market,
                &order.side,
                order.price,
                order.quantity,
                order.filled_quantity,
                &order.order_status,
                order.timestamp,
            )
            .await
            {
                tracing::error!(%e, "failed to upsert open order");
            }
        }
        Ok(DatabaseRequests::DeleteOrder { order_id }) => {
            if let Err(e) = delete_open_order(pg_pool, &order_id).await {
                tracing::error!(%e, "failed to delete open order");
            }
        }
        Ok(DatabaseRequests::UpsertBalance(balance)) => {
            if let Err(e) = upsert_user_balance(
                pg_pool,
                &balance.user_id,
                &balance.asset,
                balance.available,
                balance.locked,
            )
            .await
            {
                tracing::error!(%e, "failed to upsert user balance");
            }
        }
        Err(err) => {
            tracing::error!(%err, "failed to deserialize db request");
        }
    }
}
