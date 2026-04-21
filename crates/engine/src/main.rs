use engine::engine::engine::Engine;
use engine::order::handle_order;
use engine::user::handle_user;
use redis::{RedisManager, RedisQueues};
use sqlx_postgres::PostgresDb;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let redis_connection = Arc::new(
        RedisManager::new()
            .await
            .expect("failed to connect to Redis"),
    );

    let postgres = PostgresDb::new()
        .await
        .expect("failed to connect to Postgres");
    let pg_pool = postgres
        .get_pg_connection()
        .expect("failed to get Postgres connection pool");

    let engine = Arc::new(Mutex::new(Engine::new()));

    if let Err(e) = engine.lock().await.init_engine(&pg_pool).await {
        tracing::error!(%e, "Failed to initialize engine");
        return;
    }

    engine.lock().await.init_user_balance("test_user");

    let redis_orders = Arc::clone(&redis_connection);
    let engine_orders = Arc::clone(&engine);
    let orders_handle = task::spawn(async move {
        loop {
            match redis_orders
                .pop(&RedisQueues::Orders.to_string(), Some(1))
                .await
            {
                Ok(data) => {
                    if !data.is_empty() {
                        let mut eng = engine_orders.lock().await;
                        handle_order(data, &redis_orders, &mut eng).await;
                    }
                }
                Err(e) => tracing::error!(%e, "Error popping from orders queue"),
            }
        }
    });

    let redis_users = Arc::clone(&redis_connection);
    let engine_users = Arc::clone(&engine);
    let users_handle = task::spawn(async move {
        loop {
            match redis_users
                .pop(&RedisQueues::Users.to_string(), Some(1))
                .await
            {
                Ok(data) => {
                    if !data.is_empty() {
                        let mut eng = engine_users.lock().await;
                        handle_user(data, &redis_users, &mut eng).await;
                    }
                }
                Err(e) => tracing::error!(%e, "Error popping from users queue"),
            }
        }
    });

    if let Err(e) = orders_handle.await {
        tracing::error!(%e, "Orders task failed");
    }
    if let Err(e) = users_handle.await {
        tracing::error!(%e, "Users task failed");
    }
}
