use crate::engine::engine::Engine;
use crate::types::engine::UserRequests;
use fred::prelude::RedisValue;
use redis::RedisManager;

#[tracing::instrument(skip(data, redis_connection, engine))]
pub async fn handle_user(
    data: Vec<RedisValue>,
    redis_connection: &RedisManager,
    engine: &mut Engine,
) {
    let user_data = match data.first() {
        Some(RedisValue::String(s)) => s.to_string(),
        _ => {
            tracing::error!("Unexpected Redis value type for user request");
            return;
        }
    };

    let user_request = match serde_json::from_str::<UserRequests>(&user_data) {
        Ok(req) => req,
        Err(err) => {
            tracing::error!(%err, "Failed to deserialize user request");
            return;
        }
    };

    match user_request {
        UserRequests::CreateUser(user) => {
            let Some(pubsub_id) = user.pubsub_id else {
                tracing::error!("CreateUser missing pubsub_id");
                return;
            };
            let pubsub_id = pubsub_id.to_string();

            engine.init_user_balance(&user.user_id);
            engine
                .sync_user_balances_to_db(&user.user_id, redis_connection)
                .await;

            let response = serde_json::json!({
                "status": "Created User",
                "user_id": user.user_id,
            });

            match serde_json::to_string(&response) {
                Ok(payload) => {
                    if let Err(e) = redis_connection.publish(&pubsub_id, payload).await {
                        tracing::error!(%e, "Failed to publish user creation response");
                    }
                }
                Err(e) => tracing::error!(%e, "Failed to serialize user response"),
            }
        }
    }
}
