use actix_web::web::{Data, Json};
use actix_web::HttpResponse;
use uuid::Uuid;

use crate::types::{
    app::AppState,
    routes::{
        CancelAllOrdersInput, CancelOrderInput, CreateOrderInput, GetOpenOrderInput,
        GetOpenOrdersInput, OrderRequests,
    },
};
use redis::RedisQueues;

/// Sends a request through Redis and waits for the engine response.
async fn redis_rpc(
    app_state: &Data<AppState>,
    queue: RedisQueues,
    payload: &impl serde::Serialize,
    pubsub_id: Uuid,
) -> HttpResponse {
    let data = match serde_json::to_string(payload) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(%e, "Failed to serialize request");
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "serialization failed"
            }));
        }
    };

    match app_state
        .redis_connection
        .push_and_wait_for_subscriber(queue.to_string(), data, pubsub_id)
        .await
    {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(json) => HttpResponse::Ok().json(json),
            Err(_) => HttpResponse::Ok().body(response),
        },
        Err(e) => {
            tracing::error!(%e, "Redis RPC failed");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string()
            }))
        }
    }
}

pub async fn execute_order(
    body: Json<CreateOrderInput>,
    app_state: Data<AppState>,
) -> HttpResponse {
    let mut order = body.into_inner();

    if let Err(msg) = order.validate() {
        return HttpResponse::BadRequest().json(serde_json::json!({"error": msg}));
    }

    let pubsub_id = Uuid::new_v4();
    order.pubsub_id = Some(pubsub_id);

    let request = OrderRequests::CreateOrder(order);
    redis_rpc(&app_state, RedisQueues::Orders, &request, pubsub_id).await
}

pub async fn get_open_order(
    body: Json<GetOpenOrderInput>,
    app_state: Data<AppState>,
) -> HttpResponse {
    let mut order = body.into_inner();
    let pubsub_id = Uuid::new_v4();
    order.pubsub_id = Some(pubsub_id);

    let request = OrderRequests::GetOpenOrder(order);
    redis_rpc(&app_state, RedisQueues::Orders, &request, pubsub_id).await
}

pub async fn cancel_order(
    body: Json<CancelOrderInput>,
    app_state: Data<AppState>,
) -> HttpResponse {
    let mut order = body.into_inner();

    if let Err(msg) = order.validate() {
        return HttpResponse::BadRequest().json(serde_json::json!({"error": msg}));
    }

    let pubsub_id = Uuid::new_v4();
    order.pubsub_id = Some(pubsub_id);

    let request = OrderRequests::CancelOrder(order);
    redis_rpc(&app_state, RedisQueues::Orders, &request, pubsub_id).await
}

pub async fn get_open_orders(
    body: Json<GetOpenOrdersInput>,
    app_state: Data<AppState>,
) -> HttpResponse {
    let mut order = body.into_inner();
    let pubsub_id = Uuid::new_v4();
    order.pubsub_id = Some(pubsub_id);

    let request = OrderRequests::GetOpenOrders(order);
    redis_rpc(&app_state, RedisQueues::Orders, &request, pubsub_id).await
}

pub async fn cancel_all_orders(
    body: Json<CancelAllOrdersInput>,
    app_state: Data<AppState>,
) -> HttpResponse {
    let mut order = body.into_inner();
    let pubsub_id = Uuid::new_v4();
    order.pubsub_id = Some(pubsub_id);

    let request = OrderRequests::CancelAllOrders(order);
    redis_rpc(&app_state, RedisQueues::Orders, &request, pubsub_id).await
}
