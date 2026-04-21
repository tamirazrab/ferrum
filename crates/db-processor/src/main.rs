use db_processor::handle_db_updates;
use redis::{RedisManager, RedisQueues};
use sqlx_postgres::PostgresDb;
pub mod query;
pub mod seed;
pub mod types;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let redis_connection = RedisManager::new()
        .await
        .expect("failed to connect to Redis");

    let postgres = PostgresDb::new()
        .await
        .expect("failed to connect to Postgres");
    let pg_pool = postgres
        .get_pg_connection()
        .expect("failed to get Postgres pool");

    tracing::info!("db-processor ready");

    loop {
        match redis_connection
            .pop(&RedisQueues::Database.to_string(), Some(1))
            .await
        {
            Ok(data) => {
                if !data.is_empty() {
                    handle_db_updates(data, &pg_pool).await;
                }
            }
            Err(e) => tracing::error!(%e, "error popping from database queue"),
        }
    }
}
