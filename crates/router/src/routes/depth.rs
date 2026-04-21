use actix_web::web::Data;
use actix_web::HttpResponse;
use uuid::Uuid;

use crate::types::{
    app::AppState,
    routes::{GetDepthInput, OrderRequests},
};
use redis::RedisQueues;

pub async fn get_depth(
    query: actix_web::web::Query<GetDepthInput>,
    app_state: Data<AppState>,
) -> HttpResponse {
    let mut market_data = query.into_inner();
    let pubsub_id = Uuid::new_v4();
    market_data.pubsub_id = Some(pubsub_id);

    let request = OrderRequests::GetDepth(market_data);
    let data = match serde_json::to_string(&request) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(%e, "Failed to serialize depth request");
            return HttpResponse::InternalServerError().finish();
        }
    };

    match app_state
        .redis_connection
        .push_and_wait_for_subscriber(RedisQueues::Orders.to_string(), data, pubsub_id)
        .await
    {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(json) => HttpResponse::Ok().json(json),
            Err(_) => HttpResponse::Ok().body(response),
        },
        Err(e) => {
            tracing::error!(%e, "Failed to get depth");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string()
            }))
        }
    }
}
