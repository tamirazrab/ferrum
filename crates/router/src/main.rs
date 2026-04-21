use actix_web::{web, HttpServer};
use confik::{Configuration as _, EnvSource};
use dotenvy::dotenv;
use sqlx_postgres::PostgresDb;
use tracing_subscriber::EnvFilter;

use redis::RedisManager;
use router::config::RouterConfig;
use router::types::app::AppState;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();
    dotenv().ok();

    let config = RouterConfig::builder()
        .override_with(EnvSource::new())
        .try_build()
        .expect("failed to build router config");

    let api_key = config.api_key.clone();

    let app_state = web::Data::new(AppState {
        redis_connection: RedisManager::new()
            .await
            .expect("failed to connect to Redis"),
        postgres_db: PostgresDb::new()
            .await
            .expect("failed to connect to Postgres"),
    });

    let server = HttpServer::new(move || router::build_app(app_state.clone(), api_key.clone()))
        .bind(&config.server_addr)?
        .run();

    tracing::info!("Server running at http://{}/", config.server_addr);
    server.await
}
