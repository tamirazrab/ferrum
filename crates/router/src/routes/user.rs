use actix_web::web::Data;
use actix_web::HttpResponse;
use uuid::Uuid;

use crate::types::{
    app::AppState,
    routes::{CreateUserInput, UserRequests},
};
use redis::RedisQueues;

pub async fn create_user(app_state: Data<AppState>) -> HttpResponse {
    let user_id = Uuid::new_v4();
    let pubsub_id = Uuid::new_v4();

    let request = UserRequests::CreateUser(CreateUserInput {
        user_id: user_id.to_string(),
        pubsub_id: Some(pubsub_id),
    });

    let data = match serde_json::to_string(&request) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(%e, "Failed to serialize user request");
            return HttpResponse::InternalServerError().finish();
        }
    };

    match app_state
        .redis_connection
        .push_and_wait_for_subscriber(RedisQueues::Users.to_string(), data, pubsub_id)
        .await
    {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(json) => HttpResponse::Ok().json(json),
            Err(_) => HttpResponse::Ok().body(response),
        },
        Err(e) => {
            tracing::error!(%e, "Failed to create user via Redis RPC");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string()
            }))
        }
    }
}
