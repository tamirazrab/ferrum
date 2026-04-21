# Crate-by-Crate Guide: Reading the Exchange Code

This guide helps you navigate each service and understand the code patterns used.

---

## **1. Router Crate** (`crates/router/`)

**Purpose:** REST API gateway, receives client requests and coordinates with Engine

**Entry Points:**
- `main.rs` - Application startup
- `routes/` - HTTP endpoint handlers

### **Main Structure**

**File:** [`crates/router/src/main.rs`](../crates/router/src/main.rs)

```rust
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize Redis client
    let redis_client = create_redis_client().await;
    
    // Initialize Postgres connection pool
    let db_pool = create_db_pool().await;
    
    // Create shared app state
    let app_state = web::Data::new(AppState {
        redis: redis_client,
        db_pool,
    });
    
    // Start HTTP server
    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .configure(routes)  // Register all routes
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
```

**Key Rust Patterns:**
- `#[actix_web::main]` = Macro that sets up async runtime
- `web::Data` = Shared state across all requests
- `.app_data()` = Inject state into handlers
- `move ||` = Closure captures state (moves ownership into closure)

### **Understanding AppState**

**File:** [`crates/router/src/types/app.rs`](../crates/router/src/types/app.rs)

```rust
pub struct AppState {
    pub redis: RedisManager,     // Redis client
    pub db_pool: PgPool,         // Postgres connection pool
}

// This gets cloned for each thread in the thread pool
// Arc<AppState> is automatically created by web::Data
```

**Why?**
- Single `AppState` shared across all incoming requests
- Each worker thread gets clone of `Arc<AppState>` (cheap, just increments refcount)
- All workers can read/write to same Redis and Postgres

### **Route Handlers - Understanding Request Flow**

**File:** [`crates/router/src/routes/order.rs`](../crates/router/src/routes/order.rs)

```rust
// Handler signature - break it down:
#[post("/api/v1/order")]
pub async fn create_order(
    web::Json(payload): web::Json<CreateOrderRequest>,  // Extract JSON body
    state: web::Data<AppState>,                         // Inject app state
) -> impl Responder {                                    // Return any HTTP response
    // ...
}
```

**Mapping to TypeScript:**
```typescript
// TypeScript Express equivalent:
app.post("/api/v1/order", async (req, res) => {
    const payload = req.body;  // Automatically parsed JSON
    // res.json(response);
});

// Rust does type checking:
// - If JSON doesn't match CreateOrderRequest, returns 400 Bad Request
// - If CreateOrderRequest parsing fails, error caught at compile time
```

### **Request-Response Pattern Over Redis**

**File:** [`crates/router/src/routes/order.rs`](../crates/router/src/routes/order.rs)

```rust
pub async fn create_order(
    web::Json(payload): web::Json<CreateOrderRequest>,
    state: web::Data<AppState>,
) -> Result<web::Json<OrderResponse>, ApiError> {
    // 1. Generate unique ID for this request
    let request_id = uuid::Uuid::new_v4().to_string();
    
    // 2. Create order struct
    let order = Order {
        id: request_id.clone(),  // Clone: copies String
        user_id: payload.user_id,
        side: payload.side,
        price: payload.price,
        qty: payload.qty,
        create_time: timestamp(),
    };
    
    // 3. Subscribe to response channel
    let mut subscription = state.redis
        .subscribe(&request_id)
        .await
        .map_err(|e| ApiError::RedisError(e))?;
    
    // 4. Push order to queue
    state.redis
        .rpush("ORDERS", &order)
        .await
        .map_err(|e| ApiError::RedisError(e))?;
    
    // 5. **BLOCKING**: Wait for response (up to 5 seconds)
    let response_json = tokio::time::timeout(
        Duration::from_secs(5),
        subscription.next(),
    )
    .await
    .map_err(|_| ApiError::Timeout)?;
    
    // 6. Parse response and return
    let response: OrderResponse = serde_json::from_str(&response_json)?;
    Ok(web::Json(response))
}
```

**Timeline:**
```
T=0:      Handler receives request (blocks inside handler)
T=0-5ms:  Handler waits (Tokio pauses this task)
T=5ms:    Engine processes, publishes to request_id
T=5-10ms: Handler resumes, returns response
T=10ms:   HTTP 200 sent to client
```

**Key Rust Concepts:**
- `uuid::new_v4()` = Generate random UUID using uuid crate
- `tokio::time::timeout()` = Wrap future with timeout
- `.await` = Pause handler, let Tokio run other tasks
- `.map_err()` = Convert error type if needed

### **URL Endpoints Reference**

| Method | Path | File | Purpose |
|--------|------|------|---------|
| POST | `/api/v1/order` | `routes/order.rs` | Create new order |
| GET | `/api/v1/order/{id}` | `routes/order.rs` | Get order status |
| DELETE | `/api/v1/order/{id}` | `routes/order.rs` | Cancel order |
| GET | `/api/v1/orders` | `routes/order.rs` | Get all open orders |
| GET | `/api/v1/depth` | `routes/depth.rs` | Get order book depth |
| GET | `/api/v1/trades` | `routes/trade.rs` | Get recent trades |
| GET | `/api/v1/klines` | `routes/klines.rs` | Get candlestick data |

---

## **2. Engine Crate** (`crates/engine/`)

**Purpose:** Core order matching logic, maintains orderebook, executes trades

**Entry Points:**
- `main.rs` - Service startup
- `engine/engine.rs` - Matching algorithm
- `engine/orderbook.rs` - OrderBook data structure

### **High-Level Structure**

**File:** [`crates/engine/src/main.rs`](../crates/engine/src/main.rs)

```rust
#[tokio::main]
async fn main() {
    // Create Redis client
    let redis = create_redis_client().await;
    
    // Create Postgres pool
    let db_pool = create_db_pool().await;
    
    // Create trading engine
    let mut engine = Engine::new();
    
    // Main loop: consume orders from queue
    loop {
        // Pop order from ORDERS queue
        if let Some(order_json) = redis.lpop("ORDERS").await {
            // Parse JSON to Order struct
            let order: Order = serde_json::from_str(&order_json)?;
            
            // Process order (matching)
            match engine.create_order(order.clone()).await {
                Ok(fills) => {
                    // Publish results to three channels
                    // See section 3.1 for details
                }
                Err(e) => {
                    // Log error
                    eprintln!("Error: {:?}", e);
                    // Publish error response
                    redis.publish(&order.request_id, &error_response).await;
                }
            }
        }
    }
}
```

**Key Pattern:**
- `loop { ... }` = Infinite loop (runs until process killed)
- `if let Some(order)` = Extract Some, ignore None (order popped or not)
- `.await` on every I/O operation

### **Engine Data Structure**

**File:** [`crates/engine/src/types/engine.rs`](../crates/engine/src/types/engine.rs)

```rust
pub struct Engine {
    // For each market (SOL_USDC, BTC_USDC, etc)
    orderbooks: Vec<OrderBook>,
    
    // User account balances
    balances: HashMap<String, Arc<Mutex<UserBalances>>>,
}

pub struct OrderBook {
    // Bids: buy orders sorted by price (descending)
    // Map: price -> Vec of orders at that price
    bids: BTreeMap<Decimal, Vec<Order>>,
    
    // Asks: sell orders sorted by price (ascending)
    asks: BTreeMap<Decimal, Vec<Order>>,
    
    // For market data subscriptions
    last_update_id: u64,
}

pub struct UserBalances {
    // Asset -> locked amount
    locked: HashMap<String, Decimal>,
    
    // Asset -> available amount
    available: HashMap<String, Decimal>,
}
```

**Why these data structures?**
- `BTreeMap`: Sorted by key, O(log n) lookup
- `Vec<Order>`: Multiple orders can have same price, keep in arrival order
- `Arc<Mutex<...>>`: Multiple concurrent orders can be modifying balances safely
- `last_update_id`: For WebSocket depth subscriptions (versioning)

### **Matching Algorithm Deep Dive**

**File:** [`crates/engine/src/engine/engine.rs`](../crates/engine/src/engine/engine.rs)

```rust
pub async fn create_order(&mut self, order: Order) -> Result<Vec<Fill>, EngineError> {
    // 1. Validate user exists
    let user_balance = self.balances.get(&order.user_id)
        .ok_or(EngineError::UserNotFound)?;
    
    // 2. Lock user's balance (exclusive access)
    let mut balance = user_balance.lock().await;
    
    // 3. Verify sufficient balance
    let required = order.price * order.qty;
    let available = balance.available.get("USDC")
        .copied()
        .unwrap_or_default();
    
    if available < required {
        return Err(EngineError::InsufficientBalance { required, available });
    }
    
    // 4. Deduct from available (move to locked)
    balance.available.insert("USDC", available - required);
    balance.locked.insert("USDC", balance.locked.get("USDC").copied().unwrap_or_default() + required);
    
    // Release lock before matching (don't hold mutex while processing)
    drop(balance);
    
    // 5. Perform matching
    let mut fills = Vec::new();
    let book = &mut self.orderbooks[0];  // Currently hardcoded to SOL_USDC
    
    match order.side {
        Side::BUY => {
            // Match against asks (sellers)
            for (ask_price, asks) in book.asks.iter_mut() {
                if ask_price > &order.price {
                    break;  // Prices too high, no match
                }
                
                for ask_order in asks.iter_mut() {
                    if ask_order.qty == 0 {
                        continue;  // Already filled
                    }
                    
                    // Calculate fill quantity
                    let fill_qty = order.qty.min(ask_order.qty);
                    
                    // Create Fill (the trade record)
                    let fill = Fill {
                        id: uuid::Uuid::new_v4().to_string(),
                        buy_order_id: order.id.clone(),
                        sell_order_id: ask_order.id.clone(),
                        qty: fill_qty,
                        price: *ask_price,
                        timestamp: current_timestamp(),
                    };
                    
                    // Update order quantities
                    // ... (update both user balances)
                    
                    fills.push(fill);
                    
                    if order.qty <= 0 {
                        break;  // Buy order fully filled
                    }
                }
            }
            
            // If any quantity remains, add to book as limit order
            if order.qty > 0 {
                book.bids.entry(order.price)
                    .or_insert_with(Vec::new)
                    .push(order);
            }
        }
        
        Side::SELL => {
            // Similar logic but iterate bids
            // ...
        }
    }
    
    Ok(fills)
}
```

**Key Rust Concepts:**
- `&mut self` = Engine has exclusive access (can modify its state)
- `.lock().await` = Lock Mutex, wait if needed, returns MutexGuard
- `drop(balance)` = Explicitly release lock early (good practice)
- `for (price, orders) in book.bids.iter_mut()` = Iterate BTreeMap mutably
- `.entry(key).or_insert_with(closure)` = Idiomatic "get or insert" pattern
- `order.qty.min(ask_order.qty)` = Min of two decimals

### **Understanding Fills (Trades)**

**File:** [`crates/engine/src/types/engine.rs`](../crates/engine/src/types/engine.rs)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub id: String,
    pub buy_order_id: String,
    pub sell_order_id: String,
    pub price: Decimal,
    pub qty: Decimal,
    pub timestamp: i64,
}

// When a Fill is created:
// 1. Buy user gets SOL balance +qty
// 2. Sell user gets USDC balance +qty*price
// 3. One of them gets their locked funds released
// 4. Published to three channels:
//    - request_id (request-response)
//    - trade.SOL_USDC (WebSocket broadcast)
//    - DATABASE_QUEUE (persistence)
```

---

## **3. WS-Stream Crate** (`crates/ws-stream/`)

**Purpose:** WebSocket server, broadcasts real-time market data

**Entry Points:**
- `main.rs` - Service startup, HTTP upgrade handler
- `ws_manager.rs` - WebSocket connection management

### **Architecture**

**File:** [`crates/ws-stream/src/main.rs`](../crates/ws-stream/src/main.rs)

```rust
#[tokio::main]
async fn main() {
    let redis = create_redis_client().await;
    
    // HTTP server (upgraded to WebSocket)
    HttpServer::new(move || {
        App::new()
            .route("/ws", web::get().to(websocket_handler))
    })
    .bind("0.0.0.0:4000")?
    .run()
    .await
}

// Handler for WebSocket connections
async fn websocket_handler(
    req: HttpRequest,
    stream: web::Payload,
) -> Result<HttpResponse> {
    // Upgrade HTTP to WebSocket
    web::ws::start(WebsocketActor::new(&redis), &req, stream)
}
```

**HTTP to WebSocket Upgrade:**
```
1. Client: GET /ws
           Upgrade: websocket
           Connection: Upgrade

2. Server: 101 Switching Protocols
           Upgrade: websocket

3. Now: TCP frames are WebSocket frames (not HTTP!)
```

### **WebSocket Actor Pattern**

**File:** [`crates/ws-stream/src/ws_manager.rs`](../crates/ws-stream/src/ws_manager.rs)

```rust
pub struct WebsocketActor {
    redis: RedisManager,
    subscriptions: HashSet<String>,  // e.g., ["trade.SOL_USDC", "depth.SOL_USDC"]
}

impl Actor for WebsocketActor {
    type Context = ws::WebsocketContext<Self>;
}

// Handle incoming WebSocket messages
impl StreamHandler<Result<ws::Message, ws::error::ProtocolError>> for WebsocketActor {
    fn handle(&mut self, msg: Result<ws::Message, ws::error::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => {
                // Parse client command
                let cmd: SubscribeCommand = serde_json::from_str(&text)?;
                
                match cmd.action {
                    "subscribe" => {
                        // Add subscription
                        self.subscriptions.insert(cmd.channel.clone());
                        
                        // In background, listen to Redis pub/sub
                        let addr = ctx.address();
                        tokio::spawn(async move {
                            let mut subscription = redis.subscribe(&cmd.channel).await?;
                            while let Some(msg) = subscription.next().await {
                                // Send to WebSocket client
                                addr.do_send(WsMessage(msg));
                            }
                        });
                    }
                    "unsubscribe" => {
                        self.subscriptions.remove(&cmd.channel);
                    }
                    _ => {}
                }
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.stop();  // Close connection
            }
            _ => {}
        }
    }
}
```

**Key Pattern:**
- `Actor` pattern (message-passing concurrency)
- `StreamHandler` receives WebSocket frames
- Each connection spawns background task listening to Redis
- When Redis message arrives → `do_send()` → sends to client

### **Client Communication**

**Scenario: Client subscribes to trades**

```javascript
// Client code (TypeScript/JavaScript)
const ws = new WebSocket('ws://localhost:4000/ws');

ws.onopen = () => {
    // Subscribe to trade updates
    ws.send(JSON.stringify({
        action: 'subscribe',
        channel: 'trade.SOL_USDC'
    }));
};

ws.onmessage = (event) => {
    const message = JSON.parse(event.data);
    console.log('Trade received:', message);
    // { id: "123", buy_order_id: "...", qty: 1, price: 100, ... }
};
```

**Server internals:**
```
1. WebsocketActor receives message
2. Parses subscribe command
3. Spawns background task listening to Redis channel "trade.SOL_USDC"
4. When Engine publishes Fill to Redis → Redis sends to WS-Stream
5. WS-Stream forwards to all subscribed clients
```

---

## **4. DB-Processor Crate** (`crates/db-processor/`)

**Purpose:** Asynchronously persist trades to Postgres

**Entry Points:**
- `main.rs` - Service startup, main loop

### **Simple One-Job Service**

**File:** [`crates/db-processor/src/main.rs`](../crates/db-processor/src/main.rs)

```rust
#[tokio::main]
async fn main() {
    let redis = create_redis_client().await;
    let db_pool = create_db_pool().await;
    
    // Main loop: consume DATABASE queue
    loop {
        // Pop trade from queue
        if let Some(trade_json) = redis.lpop("DATABASE_QUEUE").await {
            // Parse into Fill struct
            let fill: Fill = serde_json::from_str(&trade_json)?;
            
            // Insert into Postgres
            match insert_trade(&db_pool, &fill).await {
                Ok(_) => {
                    println!("Inserted trade: {:?}", fill.id);
                }
                Err(e) => {
                    eprintln!("Error inserting trade: {}", e);
                    // Should retry or alert!
                }
            }
        }
        
        // Small sleep to prevent CPU spinning
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn insert_trade(pool: &PgPool, fill: &Fill) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO trades (id, buy_user_id, sell_user_id, qty, price, timestamp)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#
    )
    .bind(&fill.id)
    .bind(&fill.buy_order_id)
    .bind(&fill.sell_order_id)
    .bind(&fill.qty)
    .bind(&fill.price)
    .bind(&fill.timestamp)
    .execute(pool)
    .await?;
    
    Ok(())
}
```

**Why separate service?**
- Database writes: ~10-50ms overhead
- Order matching: must be <10ms
- Separation: If database is slow, trading still fast

**Pattern: Fire-and-Forget**
```
Engine: Publish to DATABASE_QUEUE (returns immediately)
↓ (No waiting)
DB-Processor: (Async process in background)
↓ (Can take seconds)
Postgres: Persistent storage
```

---

## **5. Redis Crate** (`crates/redis/`)

**Purpose:** Unified Redis client abstraction with pub/sub + queues

### **Key Functions**

**File:** [`crates/redis/src/lib.rs`](../crates/redis/src/lib.rs)

```rust
pub struct RedisManager {
    client: fred::RedisClient,
}

impl RedisManager {
    // Queue operations (FIFO)
    pub async fn rpush(&self, queue: &str, value: &str) -> Result<()> {
        self.client.rpush(queue, value).await?;
        Ok(())
    }
    
    pub async fn lpop(&self, queue: &str) -> Result<Option<String>> {
        Ok(self.client.lpop(queue, None).await?)
    }
    
    // Request-response pattern (the clever one!)
    pub async fn push_and_wait_for_subscriber(
        &self,
        pubsub_channel: &str,
        queue_name: &str,
        value: &str,
        timeout: Duration,
    ) -> Result<String> {
        // 1. Subscribe to response channel
        let mut subscription = self.client.subscribe(pubsub_channel).await?;
        
        // 2. Push to queue
        self.client.rpush(queue_name, value).await?;
        
        // 3. Wait for response with timeout
        let response = tokio::time::timeout(
            timeout,
            subscription.next(),
        ).await??;
        
        // 4. Return response
        Ok(response)
    }
    
    // Pub/sub
    pub async fn publish(&self, channel: &str, message: &str) -> Result<()> {
        self.client.publish(channel, message).await?;
        Ok(())
    }
}
```

**Redis Commands Used:**
- `RPUSH` - Push to right side of list (queue append)
- `LPOP` - Pop from left side of list (queue consume)
- `PUBLISH` - Send message to channel
- `SUBSCRIBE` - Listen to channel

**Queue vs Pub/Sub:**
- **Queue (RPUSH/LPOP):** Persistent, point-to-point, consumed once
- **Pub/Sub (PUBLISH/SUBSCRIBE):** Real-time, broadcast, can have multiple listeners

---

## **6. Common Utils Crate** (`crates/common_utils/`)

**Purpose:** Shared types, utilities, error types

### **Current Contents**

**File:** [`crates/common_utils/src/lib.rs`](../crates/common_utils/src/lib.rs)

```rust
pub mod types;

pub use types::{Order, Side, Fill, UserBalances};
```

**File:** [`crates/common_utils/src/types.rs`](../crates/common_utils/src/types.rs)

```rust
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub side: Side,
    pub price: Decimal,
    pub qty: Decimal,
    pub create_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Side {
    #[serde(rename = "BUY")]
    BUY,
    #[serde(rename = "SELL")]
    SELL,
}

// ... more types
```

**Why shared?**
- Engine needs Order for matching
- Router needs Order for parsing
- WS-Stream needs Fill for broadcasts
- Single source of truth for data structures

---

## **7. SQLx Postgres Crate** (`crates/sqlx_postgres/`)

**Purpose:** Database connection pool and query utilities

### **Migrations**

**File:** [`crates/sqlx_postgres/migrations/20240929080454_trades.up.sql`](../crates/sqlx_postgres/migrations/20240929080454_trades.up.sql)

```sql
CREATE TABLE IF NOT EXISTS trades (
    id SERIAL PRIMARY KEY,
    buy_user_id VARCHAR(255) NOT NULL,
    sell_user_id VARCHAR(255) NOT NULL,
    qty NUMERIC NOT NULL,
    price NUMERIC NOT NULL,
    timestamp BIGINT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_trades_buy_user ON trades(buy_user_id);
CREATE INDEX idx_trades_sell_user ON trades(sell_user_id);
CREATE INDEX idx_trades_timestamp ON trades(timestamp);
```

**SQLx Features:**
- Compile-time SQL checking
- Automatic migration running
- Connection pooling

### **Pool Creation**

**File:** [`crates/sqlx_postgres/src/lib.rs`](../crates/sqlx_postgres/src/lib.rs)

```rust
pub async fn create_pool() -> Result<PgPool> {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL not set");
    
    // Create pool (default: 10 connections)
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await?;
    
    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;
    
    Ok(pool)
}
```

**Pool Pattern:**
- Maintains ~20 persistent connections to Postgres
- Reuses connections (doesn't open/close each query)
- Much faster than creating new connection per query

---

## **Reading Sequence Recommendation**

For a TypeScript developer learning Rust:

1. **Start here:** [`rust-for-typescript-developers.md`](./rust-for-typescript-developers.md) (30 min)
   - Understand core Rust concepts
   
2. **Then read:** This file, focusing on:
   - [`#3. WS-Stream Crate`](#3-ws-stream-crate) (simpler, fewer concepts)
   - [`#5. Redis Crate`](#5-redis-crate) (abraction layer, cleaner API)
   
3. **Then read:** [`architecture-deep-dive.md`](./architecture-deep-dive.md) (45 min)
   - Understand full order flow with code
   
4. **Then deeply read:**
   - [`#2. Engine Crate`](#2-engine-crate) (most complex, most important)
   - Study `engine/engine.rs` line by line
   
5. **Finally read:** [`#1. Router Crate`](#1-router-crate)
   - How requests enter/exit system

---

## **Common Patterns You'll See**

### 1. **Struct with Derives**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order { ... }
```
- `Debug` = Can print with `{:?}`
- `Clone` = Can create copies
- `Serialize`/`Deserialize` = Can convert to/from JSON

### 2. **.await on Async Functions**
```rust
let result = async_function().await;  // MUST have .await
```
If you forget `.await`, it just returns the Future without executing!

### 3. **? Operator for Error Propagation**
```rust
let order = parse_order(&json)?;  // If error, return early
```
Shorthand for error handling

### 4. **Pattern Matching**
```rust
match order.side {
    Side::BUY => { /* ... */ }
    Side::SELL => { /* ... */ }
}
```

### 5. **Trait Bounds**
```rust
fn process<T: Serialize>(value: T) { /* ... */ }
```
Generic function only works if T implements Serialize

---

**Next Steps:**
1. Pick one crate and read all its code
2. Create a test that exercises that crate
3. Add logging to understand execution flow
4. Modify something small (add a new endpoint, change order matching logic)

See [`order-flow-walkthrough.md`](./order-flow-walkthrough.md) for step-by-step code trace!
