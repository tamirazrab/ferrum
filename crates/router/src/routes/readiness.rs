use actix_web::web::Data;
use actix_web::HttpResponse;

use crate::types::app::AppState;

/// Liveness-style check with Redis PING and Postgres `SELECT 1`.
pub async fn get_ready(app: Data<AppState>) -> HttpResponse {
    if let Err(e) = app.redis_connection.ping().await {
        tracing::warn!(%e, "readiness: redis ping failed");
        return HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "status": "unready",
            "reason": "redis"
        }));
    }

    let pool = match app.postgres_db.get_pg_connection() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(%e, "readiness: postgres pool unavailable");
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "status": "unready",
                "reason": "postgres_pool"
            }));
        }
    };

    if let Err(e) = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&pool)
        .await
    {
        tracing::warn!(%e, "readiness: postgres query failed");
        return HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "status": "unready",
            "reason": "postgres"
        }));
    }

    HttpResponse::Ok().json(serde_json::json!({ "status": "ready" }))
}
