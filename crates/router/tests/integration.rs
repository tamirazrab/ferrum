use actix_web::web;
use db_processor::handle_db_updates;
use engine::engine::engine::Engine;
use engine::order::handle_order;
use engine::user::handle_user;
use redis::{RedisManager, RedisQueues};
use reqwest::{Client, StatusCode};
use router::types::app::AppState;
use serde_json::{json, Value};
use sqlx::{Pool, Postgres};
use sqlx_postgres::PostgresDb;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

fn integration_api_key() -> String {
    std::env::var("API_KEY").unwrap_or_else(|_| "test-integration-key".to_string())
}

struct TestHarness {
    base_url: String,
    client: Client,
    api_key: String,
    _engine_orders: tokio::task::JoinHandle<()>,
    _engine_users: tokio::task::JoinHandle<()>,
    _db_processor: tokio::task::JoinHandle<()>,
}

async fn teardown_data(pool: &Pool<Postgres>, redis: &RedisManager) {
    let _ = sqlx::query(
        "TRUNCATE trades, open_orders, user_balances RESTART IDENTITY CASCADE",
    )
    .execute(pool)
    .await;
    let _ = redis.reset_exchange_queues().await;
}

async fn setup() -> TestHarness {
    let _ = dotenvy::dotenv();

    let postgres = PostgresDb::new().await.expect("Postgres connection failed");
    let pool = postgres
        .get_pg_connection()
        .expect("Postgres pool failed");

    let redis_engine = Arc::new(RedisManager::new().await.expect("Redis connection failed"));
    teardown_data(&pool, redis_engine.as_ref()).await;

    let engine = Arc::new(Mutex::new(Engine::new()));
    engine
        .lock()
        .await
        .init_engine(&pool)
        .await
        .expect("Engine init failed");

    let redis_orders = Arc::clone(&redis_engine);
    let engine_orders = Arc::clone(&engine);
    let orders_handle = tokio::spawn(async move {
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
                Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    });

    let redis_users = Arc::clone(&redis_engine);
    let engine_users = Arc::clone(&engine);
    let users_handle = tokio::spawn(async move {
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
                Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    });

    let redis_db = RedisManager::new().await.expect("Redis for db-processor failed");
    let pool_db = pool.clone();
    let db_handle = tokio::spawn(async move {
        loop {
            match redis_db
                .pop(&RedisQueues::Database.to_string(), Some(1))
                .await
            {
                Ok(data) => {
                    if !data.is_empty() {
                        handle_db_updates(data, &pool_db).await;
                    }
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    });

    let app_redis = RedisManager::new().await.expect("Redis connection for router failed");
    let app_postgres = PostgresDb::new().await.expect("Postgres for router failed");

    let app_state = web::Data::new(AppState {
        redis_connection: app_redis,
        postgres_db: app_postgres,
    });

    let api_key = integration_api_key();

    let server = actix_web::HttpServer::new({
        let api_key = api_key.clone();
        move || router::build_app(app_state.clone(), api_key.clone())
    })
    .bind("127.0.0.1:0")
    .expect("Failed to bind test server");

    let addrs = server.addrs();
    let addr = addrs[0];
    let server_handle = server.run();
    tokio::spawn(server_handle);

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest client");

    let base_url = format!("http://{}", addr);

    TestHarness {
        base_url,
        client,
        api_key,
        _engine_orders: orders_handle,
        _engine_users: users_handle,
        _db_processor: db_handle,
    }
}

impl TestHarness {
    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base_url, path)
    }

    fn authed_get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .get(self.url(path))
            .header("X-API-Key", &self.api_key)
    }

    fn authed_post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .post(self.url(path))
            .header("X-API-Key", &self.api_key)
    }

    fn authed_delete(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .delete(self.url(path))
            .header("X-API-Key", &self.api_key)
    }
}

#[actix_rt::test]
async fn health_check_no_auth() {
    let harness = setup().await;

    let resp = harness
        .client
        .get(harness.url("/health"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[actix_rt::test]
async fn ready_no_auth_checks_dependencies() {
    let harness = setup().await;

    let resp = harness
        .client
        .get(harness.url("/ready"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ready");
}

#[actix_rt::test]
async fn auth_rejection_without_api_key() {
    let harness = setup().await;

    let resp = harness
        .client
        .post(harness.url("/users"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("API key"));
}

#[actix_rt::test]
async fn create_user_via_rpc() {
    let harness = setup().await;

    let resp = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "Created User");
    assert!(body["user_id"].as_str().is_some());
}

#[actix_rt::test]
async fn place_and_retrieve_order() {
    let harness = setup().await;

    let user_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let user_id = user_resp["user_id"].as_str().unwrap().to_string();

    let order_resp = harness
        .authed_post("/order")
        .json(&json!({
            "market": "SOL_USDC",
            "price": "50.00",
            "quantity": "10",
            "side": "Buy",
            "user_id": user_id
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(order_resp.status(), StatusCode::OK);
    let order_body: Value = order_resp.json().await.unwrap();
    assert_eq!(order_body["status"], "Created Order");
    let order_id = order_body["order_id"].as_str().unwrap().to_string();

    let get_resp = harness
        .authed_get("/order")
        .json(&json!({
            "user_id": user_id,
            "order_id": order_id,
            "market": "SOL_USDC"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body: Value = get_resp.json().await.unwrap();
    assert_eq!(get_body["order_id"], order_id);
    assert_eq!(get_body["user_id"], user_id);
}

#[actix_rt::test]
async fn list_open_orders() {
    let harness = setup().await;

    let user_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let user_id = user_resp["user_id"].as_str().unwrap().to_string();

    for price in &["50.00", "60.00"] {
        harness
            .authed_post("/order")
            .json(&json!({
                "market": "SOL_USDC",
                "price": price,
                "quantity": "5",
                "side": "Buy",
                "user_id": user_id
            }))
            .send()
            .await
            .unwrap();
    }

    let resp = harness
        .authed_get("/orders")
        .json(&json!({
            "user_id": user_id,
            "market": "SOL_USDC"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let orders: Value = resp.json().await.unwrap();
    assert!(orders.as_array().unwrap().len() >= 2);
}

#[actix_rt::test]
async fn cancel_order_flow() {
    let harness = setup().await;

    let user_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let user_id = user_resp["user_id"].as_str().unwrap().to_string();

    let order_resp: Value = harness
        .authed_post("/order")
        .json(&json!({
            "market": "SOL_USDC",
            "price": "50.00",
            "quantity": "5",
            "side": "Buy",
            "user_id": user_id
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let order_id = order_resp["order_id"].as_str().unwrap().to_string();

    let cancel_resp = harness
        .authed_delete("/order")
        .json(&json!({
            "order_id": order_id,
            "user_id": user_id,
            "price": "50.00",
            "side": "Buy",
            "market": "SOL_USDC"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(cancel_resp.status(), StatusCode::OK);
    let cancel_body: Value = cancel_resp.json().await.unwrap();
    assert_eq!(cancel_body["status"], "Cancelled Order");
}

#[actix_rt::test]
async fn cancel_all_orders_flow() {
    let harness = setup().await;

    let user_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let user_id = user_resp["user_id"].as_str().unwrap().to_string();

    for _ in 0..2 {
        harness
            .authed_post("/order")
            .json(&json!({
                "market": "SOL_USDC",
                "price": "50.00",
                "quantity": "3",
                "side": "Buy",
                "user_id": user_id
            }))
            .send()
            .await
            .unwrap();
    }

    let resp = harness
        .authed_delete("/orders")
        .json(&json!({
            "user_id": user_id,
            "market": "SOL_USDC"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "Cancelled All Orders");
}

#[actix_rt::test]
async fn depth_after_order() {
    let harness = setup().await;

    let user_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let user_id = user_resp["user_id"].as_str().unwrap().to_string();

    harness
        .authed_post("/order")
        .json(&json!({
            "market": "SOL_USDC",
            "price": "50.00",
            "quantity": "10",
            "side": "Buy",
            "user_id": user_id
        }))
        .send()
        .await
        .unwrap();

    let resp = harness
        .authed_get("/depth")
        .query(&[("symbol", "SOL_USDC")])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["bids"].as_array().is_some());
    assert!(body["asks"].as_array().is_some());
}

#[actix_rt::test]
async fn full_trade_flow() {
    let harness = setup().await;

    let seller_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let seller_id = seller_resp["user_id"].as_str().unwrap().to_string();

    let buyer_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let buyer_id = buyer_resp["user_id"].as_str().unwrap().to_string();

    let sell_resp: Value = harness
        .authed_post("/order")
        .json(&json!({
            "market": "SOL_USDC",
            "price": "100.00",
            "quantity": "5",
            "side": "Sell",
            "user_id": seller_id
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(sell_resp["status"], "Created Order");

    let buy_resp: Value = harness
        .authed_post("/order")
        .json(&json!({
            "market": "SOL_USDC",
            "price": "100.00",
            "quantity": "5",
            "side": "Buy",
            "user_id": buyer_id
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(buy_resp["status"], "Created Order");

    let depth_resp: Value = harness
        .authed_get("/depth")
        .query(&[("symbol", "SOL_USDC")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(depth_resp["bids"].as_array().unwrap().is_empty());
    assert!(depth_resp["asks"].as_array().unwrap().is_empty());
}

#[actix_rt::test]
async fn get_trades_after_match() {
    let harness = setup().await;

    let seller_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let seller_id = seller_resp["user_id"].as_str().unwrap().to_string();

    let buyer_resp: Value = harness
        .authed_post("/users")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let buyer_id = buyer_resp["user_id"].as_str().unwrap().to_string();

    harness
        .authed_post("/order")
        .json(&json!({
            "market": "SOL_USDC",
            "price": "42.00",
            "quantity": "2",
            "side": "Sell",
            "user_id": seller_id
        }))
        .send()
        .await
        .unwrap();

    harness
        .authed_post("/order")
        .json(&json!({
            "market": "SOL_USDC",
            "price": "42.00",
            "quantity": "2",
            "side": "Buy",
            "user_id": buyer_id
        }))
        .send()
        .await
        .unwrap();

    let mut trades_len = 0;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let resp = harness
            .authed_get("/trades")
            .query(&[("symbol", "SOL_USDC")])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let arr: Vec<Value> = resp.json().await.unwrap();
        trades_len = arr.len();
        if trades_len >= 1 {
            break;
        }
    }

    assert!(
        trades_len >= 1,
        "expected at least one trade in Postgres after match"
    );
}

#[actix_rt::test]
async fn depth_unknown_market_returns_within_rpc_timeout() {
    let harness = setup().await;

    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap();

    let start = std::time::Instant::now();
    let resp = client
        .get(format!("{}/api/v1/depth", harness.base_url))
        .header("X-API-Key", &harness.api_key)
        .query(&[("symbol", "UNKNOWN_PAIR_XYZ")])
        .send()
        .await
        .expect("request should complete (engine returns empty depth, no hang)");
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(body["bids"].as_array().unwrap().is_empty());
    assert!(body["asks"].as_array().unwrap().is_empty());
    assert!(
        start.elapsed() < Duration::from_secs(7),
        "expected response well under Redis RPC timeout"
    );
}
