use actix_web::web::Data;
use actix_web::HttpResponse;
use db_processor::query::get_tickers_from_db;

use crate::types::app::AppState;

pub async fn get_tickers(app_state: Data<AppState>) -> HttpResponse {
    let pg_pool = match app_state.postgres_db.get_pg_connection() {
        Ok(pool) => pool,
        Err(e) => {
            tracing::error!(%e, "Failed to get Postgres connection");
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database connection failed"
            }));
        }
    };

    match get_tickers_from_db(&pg_pool).await {
        Ok(tickers) => HttpResponse::Ok().json(tickers),
        Err(e) => {
            tracing::error!(%e, "Failed to fetch tickers");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "failed to fetch tickers"
            }))
        }
    }
}
