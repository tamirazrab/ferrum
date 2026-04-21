use crate::engine::engine::Engine;
use crate::types::engine::OrderRequests;
use fred::prelude::RedisValue;
use redis::RedisManager;

fn redis_value_to_string(value: &RedisValue) -> Option<String> {
    match value {
        RedisValue::String(s) => Some(s.to_string()),
        RedisValue::Bytes(b) => String::from_utf8(b.to_vec()).ok(),
        _ => None,
    }
}

#[tracing::instrument(skip(data, redis_connection, engine))]
pub async fn handle_order(
    data: Vec<RedisValue>,
    redis_connection: &RedisManager,
    engine: &mut Engine,
) {
    let Some(order_data) = data.first().and_then(redis_value_to_string) else {
        tracing::error!("Failed to extract order data from Redis");
        return;
    };

    let order_request = match serde_json::from_str::<OrderRequests>(&order_data) {
        Ok(req) => req,
        Err(err) => {
            tracing::error!(%err, "Failed to deserialize order request");
            return;
        }
    };

    match order_request {
        OrderRequests::CreateOrder(order) => {
            let Some(pubsub_id) = order.pubsub_id else {
                tracing::error!("CreateOrder missing pubsub_id");
                return;
            };
            let pubsub_id = pubsub_id.to_string();

            let response = match engine.create_order(order, redis_connection).await {
                Ok(order_id) => serde_json::json!({
                    "status": "Created Order",
                    "order_id": order_id,
                }),
                Err(e) => serde_json::json!({
                    "status": "Failed to Create Order",
                    "error": e.to_string(),
                }),
            };

            publish_response(redis_connection, &pubsub_id, &response).await;
        }

        OrderRequests::GetOpenOrder(open_order) => {
            let Some(pubsub_id) = open_order.pubsub_id else {
                tracing::error!("GetOpenOrder missing pubsub_id");
                return;
            };
            let pubsub_id = pubsub_id.to_string();

            let response = match engine.get_open_order(open_order) {
                Ok(order) => serde_json::json!(order),
                Err(_) => serde_json::json!({
                    "status": "Failed to Retrieve Open Order",
                }),
            };

            publish_response(redis_connection, &pubsub_id, &response).await;
        }

        OrderRequests::CancelOrder(cancel_order) => {
            let Some(pubsub_id) = cancel_order.pubsub_id else {
                tracing::error!("CancelOrder missing pubsub_id");
                return;
            };
            let pubsub_id = pubsub_id.to_string();

            let response =
                match engine
                    .cancel_order(cancel_order, Some(redis_connection))
                    .await
                {
                    Ok(order_id) => serde_json::json!({
                        "status": "Cancelled Order",
                        "order_id": order_id,
                    }),
                    Err(e) => serde_json::json!({
                        "status": "Failed to Cancel Order",
                        "error": e.to_string(),
                    }),
                };

            publish_response(redis_connection, &pubsub_id, &response).await;
        }

        OrderRequests::GetOpenOrders(open_orders) => {
            let Some(pubsub_id) = open_orders.pubsub_id else {
                tracing::error!("GetOpenOrders missing pubsub_id");
                return;
            };
            let pubsub_id = pubsub_id.to_string();

            let orders = engine.get_open_orders(open_orders);
            let response = serde_json::json!(orders);

            publish_response(redis_connection, &pubsub_id, &response).await;
        }

        OrderRequests::CancelAllOrders(cancel_all_orders) => {
            let Some(pubsub_id) = cancel_all_orders.pubsub_id else {
                tracing::error!("CancelAllOrders missing pubsub_id");
                return;
            };
            let user_id = cancel_all_orders.user_id.clone();
            let pubsub_id = pubsub_id.to_string();

            let response = match engine
                .cancel_all_orders(cancel_all_orders, Some(redis_connection))
                .await
            {
                Ok(_) => serde_json::json!({
                    "status": "Cancelled All Orders",
                    "user_id": user_id,
                }),
                Err(e) => serde_json::json!({
                    "status": "Failed to Cancel All Orders",
                    "error": e.to_string(),
                }),
            };

            publish_response(redis_connection, &pubsub_id, &response).await;
        }

        OrderRequests::GetDepth(depth) => {
            let Some(pubsub_id) = depth.pubsub_id else {
                tracing::error!("GetDepth missing pubsub_id");
                return;
            };
            let pubsub_id = pubsub_id.to_string();

            let depth_result = engine.get_depth(depth);
            let response = serde_json::json!({
                "bids": depth_result.0,
                "asks": depth_result.1,
            });

            publish_response(redis_connection, &pubsub_id, &response).await;
        }
    }
}

async fn publish_response(
    redis_conn: &RedisManager,
    channel: &str,
    response: &serde_json::Value,
) {
    match serde_json::to_string(response) {
        Ok(payload) => {
            if let Err(e) = redis_conn.publish(channel, payload).await {
                tracing::error!(channel = %channel, error = %e, "Failed to publish response");
            }
        }
        Err(e) => tracing::error!(%e, "Failed to serialize response"),
    }
}
