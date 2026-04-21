use actix_web::web::Data;
use actix_web::HttpResponse;
use db_processor::query::get_trades_from_db;

use crate::types::{app::AppState, routes::GetTradesInput};

pub async fn get_trades(
    query: actix_web::web::Query<GetTradesInput>,
    app_state: Data<AppState>,
) -> HttpResponse {
    let market_data = query.into_inner();

    let pg_pool = match app_state.postgres_db.get_pg_connection() {
        Ok(pool) => pool,
        Err(e) => {
            tracing::error!(%e, "Failed to get Postgres connection");
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database connection failed"
            }));
        }
    };

    match get_trades_from_db(&pg_pool, market_data.symbol).await {
        Ok(trades) => HttpResponse::Ok().json(trades),
        Err(e) => {
            tracing::error!(%e, "Failed to fetch trades");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "failed to fetch trades"
            }))
        }
    }
}
