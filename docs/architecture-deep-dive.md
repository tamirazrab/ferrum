# Architecture Deep Dive: How It All Fits Together

This document explains the exchange architecture with code references and reasoning behind design decisions.

---  

## **1. System Overview: Three Patterns in Action**

The exchange uses three fundamental patterns which you should recognize:

### Pattern 1: Request-Response over Redis (Sync-like over Async)
**In TypeScript, you'd use:**
```typescript
// HTTP Request-Response (synchronous-looking)
const response = await fetch('/api/orders', { method: 'POST', body });
```

**In Rust Exchange:**
```rust
// Generate UUID for tracking
let response_channel_id = uuid::Uuid::new_v4().to_string();

// Subscribe to channel (will wait here)
redis_client.subscribe(&response_channel_id).await;

// Publish order to queue
redis_client.rpush("ORDERS", order_data).await;

// Blocks until response published
let response = redis_client.wait_for_pubsub(&response_channel_id, TIMEOUT).await?;
```

**Why this pattern?**
- Over HTTP: Router ↔ Engine would need network calls (latency!)
- Over Redis: In-memory, but still decoupled (Engine doesn't block Router)
- Mimics synchronous API for clients while keeping system async internally

See: [`crates/redis/src/lib.rs`](../crates/redis/src/lib.rs) - `push_and_wait_for_subscriber()`

---

### Pattern 2: Fan-Out Broadcast (Publish-Subscribe)
**In TypeScript:**
```typescript
// Socket.io rooms
io.to('trade.SOL_USDC').emit('trade', tradeData);
```

**In Rust Exchange:**
```rust
// Redis pub/sub - Engine publishes to topic
redis_client.publish("trade.SOL_USDC", serialized_trade).await;

// WS-Stream subscribed to topic (in background)
while let Some(message) = subscription.next().await {
    // Broadcast to all connected WebSocket clients in that room
    broadcast_to_subscribers(&message).await;
}
```

**Why this pattern?**
- Decouples: Engine doesn't know about WebSocket connections
- Scalability: Can add multiple WS-Stream instances
- Real-time: Sub-millisecond latency for market data

See: [`crates/ws-stream/src/ws_manager.rs`](../crates/ws-stream/src/ws_manager.rs) - subscription loop

---

### Pattern 3: Fire-and-Forget with Queue (Async Persistence)
**In TypeScript:**
```typescript
// Don't wait for database
setTimeout(() => {
    db.insert('trades', tradeData);  // Will run later, fire-and-forget
}, 0);
```

**In Rust Exchange:**
```rust
// Engine: Publish trade to queue, don't wait
redis_client.rpush("DATABASE_QUEUE", serialized_trade).await;
// ← Returns immediately, no waiting for DB

// DB-Processor (separate process): Continuously processes queue
while let Some(trade) = redis_queue.lpop("DATABASE_QUEUE").await {
    postgres.insert_trade(trade).await;  // Can take 10ms, doesn't block trading
}
```

**Why this pattern?**
- Engine: Latency-critical (< 10ms per order)
- Database: I/O-bound (50ms+ per write)
- Separation: DB failure doesn't crash trading

See: [`crates/engine/src/main.rs`](../crates/engine/src/main.rs) - publish to DATABASE queue
See: [`crates/db-processor/src/main.rs`](../crates/db-processor/src/main.rs) - consume queue

---

## **2. Data Flow: Full Order Lifecycle**

Let's trace a complete order through the system:

### 1️⃣ **User Submits Order** → Router

**File:** [`crates/router/src/main.rs`](../crates/router/src/main.rs)

```rust
// Entry point (HTTP handler)
#[post("/api/v1/order")]
async fn create_order(
    web::Json(order_input): web::Json<CreateOrderInput>,
    state: web::Data<AppState>,
) -> impl Responder {
    // Your request arrives here
```

**What Router does:**
```rust
// 1. Generate unique ID for response tracking
let pubsub_id = uuid::Uuid::new_v4().to_string();

// 2. Subscribe to Redis channel (will wait here for response)
state.redis.subscribe(&pubsub_id).await;

// 3. Serialize the order
let order_json = serde_json::to_string(&order_input)?;

// 4. Push to ORDERS queue
state.redis.rpush("ORDERS", &order_json).await?;

// 5. **BLOCKING**: Wait for Engine to process and respond
let response = state.redis
    .wait_on_pubsub(&pubsub_id, Duration::from_secs(5))
    .await?;  // If timeout, error

// 6. Return HTTP 200 to client
HttpResponse::Ok().json(response)
```

**Key Rust concept:** `async fn` + `await`
- If you didn't have `await`, would just return Future (not execute)
- `await` pauses Router handler, resumes when response arrives
- Meanwhile, Tokio runtime can process other incoming requests

---

### 2️⃣ **Engine Processes Order** → Matching

**File:** [`crates/engine/src/main.rs`](../crates/engine/src/main.rs), [`crates/engine/src/engine/engine.rs`](../crates/engine/src/engine/engine.rs)

```rust
// Engine's main loop
loop {
    // 1. Pop order from ORDERS queue
    if let Some(order_json) = redis.lpop("ORDERS").await {
        let order: Order = serde_json::from_str(&order_json)?;
        
        // 2. Match it
        let fills = matching_engine.create_order(order).await?;
        
        // 3. Publish results (see step 3)
    }
}
```

**Inside matching algorithm:**

```rust
pub async fn create_order(&mut self, order: Order) -> Result<Vec<Fill>, EngineError> {
    // Get the orderbook for this market (e.g., SOL_USDC)
    let book = &mut self.orderbooks[0];  // Currently hardcoded ⚠️
    
    // Get user's balance
    let mut balance = self.balances.get_mut(&order.user_id)
        .ok_or(EngineError::UserNotFound)?;
    
    // LOCK IT (exclusive access)
    let mut balance_lock = balance.lock().await;
    
    // Verify sufficient balance for order
    if balance_lock.get_available("USDC") < order.price * order.qty {
        return Err(EngineError::InsufficientBalance);
    }
    
    // DEDUCT from balance (locked funds)
    balance_lock.lock_funds("USDC", order.price * order.qty);
    
    // Drop the lock (balance is unlocked)
    drop(balance_lock);
    
    // Now try to match
    let mut fills = Vec::new();
    
    if order.side == Side::BUY {
        // Match against sells (lowest price first)
        for (price, asks) in book.asks.iter() {
            for ask_order in asks.iter_mut() {
                if ask_order.qty > 0 && ask_order.price <= order.price {
                    // FILL: Create Fill struct
                    let fill = Fill {
                        buy_order_id: order.id.clone(),
                        sell_order_id: ask_order.id.clone(),
                        qty: ask_order.qty.min(order.qty),
                        price: ask_order.price,
                        // ...
                    };
                    fills.push(fill);
                    
                    // Update both users' balances
                    // ...
                }
            }
        }
    }
    
    Ok(fills)
}
```

**Key Rust concepts here:**
- `&mut self` = Engine owns data, function gets exclusive access
- `.lock().await` = Mutex (exclusive access to user balance)
- `drop(balance_lock)` = Release lock early (good practice, don't hold unnecessarily)
- Pattern matching: `if order.side == Side::BUY`
- Iterator pattern: `for (price, asks) in book.asks.iter()`

---

### 3️⃣ **Engine Publishes Results** → Multiple Channels

**File:** [`crates/engine/src/main.rs`](../crates/engine/src/main.rs)

Engine publishes to THREE channels simultaneously:

```rust
// After matching is complete (from step 2)
for fill in fills.iter() {
    // Channel 1: Respond to original request
    let response = OrderResponse {
        status: "FILLED",
        order_id: fill.buy_order_id.clone(),
        // ...
    };
    redis.publish(&pubsub_id, serde_json::to_string(&response)?).await?;
    
    // Channel 2: Broadcast to market data subscribers
    redis.publish(
        &format!("trade.SOL_USDC"),
        serde_json::to_string(&fill)?
    ).await?;
    
    // Channel 3: Queue for persistent storage
    redis.rpush(
        "DATABASE_QUEUE",
        serde_json::to_string(&fill)?
    ).await?;
}
```

**Why three channels?**
- **pubsub_id**: Router gets response → sends HTTP 200 to client (request-response)
- **trade.SOL_USDC**: WS-Stream gets notification → broadcasts to all subscribers (pub-sub)
- **DATABASE_QUEUE**: DB-Processor gets trade → writes to Postgres (fire-and-forget)

---

### 4️⃣ **WS-Stream Broadcasts Trade** → Connected Clients

**File:** [`crates/ws-stream/src/main.rs`](../crates/ws-stream/src/main.rs), [`crates/ws-stream/src/ws_manager.rs`](../crates/ws-stream/src/ws_manager.rs)

```rust
// WS-Stream subscribed to trade.SOL_USDC channel
let mut subscription = redis.subscribe("trade.SOL_USDC").await?;

// Listen for trades (runs continuously)
while let Some(message) = subscription.next().await {
    let trade: Fill = serde_json::from_str(&message)?;
    
    // Broadcast to all WebSocket clients subscribed to this market
    for (client_id, _) in user_subscriptions.iter() {
        // Send WebSocket frame to this client
        send_to_websocket(client_id, &trade).await?;
    }
}
```

**Data flow for WebSocket:**

```rust
// Client connects via WebSocket
async fn handle_ws_connection(
    ws_stream: WebSocketStream,
    user_id: String,
) {
    let (tx, mut rx) = ws_stream.split();  // Split into sender + receiver
    
    // Listen for client messages
    while let Some(msg) = rx.next().await {
        match msg {
            // Client subscribes: { "cmd": "subscribe", "channel": "trade.SOL_USDC" }
            WebSocketMessage::Text(text) => {
                let cmd: SubscribeCmd = serde_json::from_str(&text)?;
                
                // Add to user_subscriptions map
                user_subscriptions.insert(user_id.clone(), cmd.channel.clone());
                
                // When trade published, this client gets it
            }
            // ...
        }
    }
}

// When trade received from Redis (see step 4)
for (client_id, subscribed_channel) in user_subscriptions.iter() {
    if subscribed_channel.contains(&trade.market) {
        // Send to this client's WebSocket
        tx.send(WebSocketMessage::Text(
            serde_json::to_string(&trade)?
        )).await?;
    }
}
```

**Key Rust concepts:**
- `split()` = Split stream into sender (tx) and receiver (rx)
- `while let Some(msg) = rx.next().await` = Loop receiving messages (async)
- `.next().await` = Wait for next message (pauses handler)
- Multiple clients handled concurrently (Tokio spawns task for each)

---

### 5️⃣ **DB-Processor Persists Trade** → Postgres

**File:** [`crates/db-processor/src/main.rs`](../crates/db-processor/src/main.rs)

```rust
// Main loop
loop {
    // Pop from queue
    if let Some(trade_json) = redis.lpop("DATABASE_QUEUE").await {
        let fill: Fill = serde_json::from_str(&trade_json)?;
        
        // Insert into Postgres
        sqlx::query(
            r#"
            INSERT INTO trades (trade_id, market, price, quantity, user_id, other_user_id, order_id, timestamp)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#
        )
        .bind(&fill.id)
        .bind(&fill.market)
        .bind(&fill.price)
        .bind(&fill.qty)
        .bind(&fill.buy_order_id)
        .bind(&fill.sell_order_id)
        .bind(&fill.market)
        .bind(&fill.timestamp)
        .execute(&postgres_pool)
        .await?;
    }
}
```

**Why separate process?**
- Database writes can take 10-50ms
- If Engine waited, latency would be terrible
- DB-Processor can batch writes (optimization)
- If Postgres is slow, trading still fast

**Key Rust concepts:**
- `sqlx::query()` = Compile-time checked SQL (errors caught at build time!)
- `.bind()` = Type-safe parameter binding (prevents SQL injection)
- `.execute()` = Actually run the query
- All `.await` calls (it's async)

---

## **3. Memory Model: How Data Flows Through Tokio**

### The Tokio Runtime

```rust
#[tokio::main]  // Macro that creates Tokio runtime
async fn main() {
    // When you write async fn main(), Tokio:
    // 1. Creates executor (thread pool)
    // 2. Spawns main as first task
    // 3. Runs all tasks concurrently
    
    // Router service
    tokio::spawn(async {
        start_router().await
    });
    
    // Engine service
    tokio::spawn(async {
        start_engine().await
    });
    
    // WS-Stream service
    tokio::spawn(async {
        start_ws_stream().await
    });
    
    // All three run "at the same time" (multiplexed)
}
```

**How concurrent requests work:**

```
Timeline:
──────────────────────────────────────────────────────────

T=0ms:    Request1 arrives → Router spawns task1
T=0.1ms:  Request2 arrives → Router spawns task2
T=0.2ms:  Task1 awaits Redis → yields control to Tokio
T=0.3ms:  Task2 awaits Redis → yields control to Tokio
T=5ms:    Redis responds to Task1 → Tokio resumes Task1
T=5.5ms:  Task1 sends HTTP response
T=10ms:   Redis responds to Task2 → Tokio resumes Task2
T=10.5ms: Task2 sends HTTP response

Result: Both requests processed "concurrently" with minimal overhead
```

**Key insight:** Without `.await`, nothing happens!

```rust
// ❌ WRONG: Creates thread, not awaited
tokio::spawn(async {
    start_router();  // No await! Won't actually run
});

// ✓ CORRECT: Creates task, awaited
tokio::spawn(async {
    start_router().await;  // Will run
});
```

---

## **4. Type Safety at Compile Time**

### Example: Order Types

**File:** [`crates/engine/src/types/engine.rs`](../crates/engine/src/types/engine.rs)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub side: Side,           // BUY or SELL
    pub price: Decimal,       // Decimal, not f64 (precision!)
    pub qty: Decimal,
    pub create_time: i64,     // Unix timestamp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Side {
    BUY,
    SELL,
}
```

**Compile-time safety:**

```rust
// ✅ Correct: Rust knows Order structure at compile time
let order = Order {
    id: "123".to_string(),
    user_id: "user1".to_string(),
    side: Side::BUY,
    price: Decimal::new(100, 0),
    qty: Decimal::new(1, 0),
    create_time: now(),
};

// ❌ Wrong: Missing field → ERROR at compile time
let order = Order {
    id: "123".to_string(),
    // user_id missing! ERROR!
};

// ❌ Wrong: Wrong type → ERROR at compile time
let order = Order {
    id: "123".to_string(),
    user_id: "user1".to_string(),
    side: "BUY",  // Should be Side::BUY, ERROR!
    // ...
};
```

**Why this matters:**
- TypeScript: These errors caught at runtime (crash in production)
- Rust: These errors prevented before shipping

---

## **5. Error Propagation with `?` Operator**

### The Problem: Nested Error Handling

```typescript
// TypeScript: Error handling is verbose
async function processOrder(orderId: string) {
    try {
        const order = await db.getOrder(orderId);
        if (!order) throw new Error("Order not found");
        
        const balance = await db.getBalance(order.userId);
        if (!balance) throw new Error("User not found");
        
        if (balance < order.amount) throw new Error("Insufficient balance");
        
        await db.deductBalance(order.userId, order.amount);
        await db.insertTrade(order);
        
        return { success: true };
    } catch (e) {
        console.error("Error:", e);
        throw e;
    }
}
```

### The Solution: `?` Operator in Rust

```rust
async fn process_order(order_id: &str) -> Result<OrderResponse, ProcessError> {
    // Each `?` = if Ok, unwrap; if Err, return error early
    let order = db.get_order(order_id).await?;  // If Err, returns early
    let balance = db.get_balance(&order.user_id).await?;  // If Err, returns early
    
    if balance < order.amount {
        return Err(ProcessError::InsufficientBalance);
    }
    
    db.deduct_balance(&order.user_id, order.amount).await?;
    db.insert_trade(&order).await?;
    
    Ok(OrderResponse { success: true })
}

// Usage:
match process_order("order1").await {
    Ok(response) => println!("Success: {:?}", response),
    Err(e) => eprintln!("Error: {:?}", e),  // MUST handle error
}
```

**Key difference:**
- TypeScript: Error handling optional (can ignore with `.catch(() => {})`)
- Rust: Compiler forces you to handle every error
- `?` = shorthand for early return on error

---

## **6. Shared State & Concurrency**

### The Challenge: Multiple tasks updating same data

```typescript
// TypeScript: Possible data race
let orderCount = 0;

async function addOrder(order) {
    orderCount += 1;  // What if two addOrder calls happen simultaneously?
    // Race condition: might end up with orderCount = 1 instead of 2
}
```

### Rust Solution: `Arc<Mutex<T>>`

```rust
// Arc = Atomically Reference Counted (shared ownership)
// Mutex = Mutual Exclusion (only one can access at a time)

let order_count = Arc::new(Mutex::new(0_u64));

// Share with multiple tasks
for i in 0..10 {
    let count = Arc::clone(&order_count);
    tokio::spawn(async move {
        // Lock the mutex (waits if another task holds it)
        let mut locked_count = count.lock().await;
        *locked_count += 1;  // Safe: only one task here
        // Lock released when locked_count goes out of scope
    });
}
```

**In exchange:**

```rust
// File: crates/engine/src/engine/engine.rs
pub struct Engine {
    // User balances protected by Arc<Mutex>
    balances: HashMap<String, Arc<Mutex<UserBalances>>>,
}

impl Engine {
    pub async fn create_order(&mut self, order: Order) -> Result<Vec<Fill>, Error> {
        // Get lock on user's balance
        let mut balance = self.balances.get(&order.user_id)
            .unwrap()
            .lock()
            .await;
        
        // Modify it (exclusive access)
        balance.usdc -= order.amount;
        
        // Lock released here
        
        Ok(fills)
    }
}
```

**Why Arc<Mutex>?**
- `Arc` = Multiple tasks can own the same data (reference counting)
- `Mutex` = Only one task modifies at a time (prevents race conditions)
- Together: Shared, safe concurrency

---

## **7. Performance Optimizations**

### Stack vs Heap

```rust
// Stack allocation (fast, automatic cleanup)
let order = Order {
    id: "123".to_string(),
    user_id: "user1".to_string(),
    // Small structs on stack = fast!
};

// Heap allocation (slower, manual cleanup in Rust)
let orders = vec![order1, order2, order3];  // Vec is heap-allocated
// But Rust cleans up automatically when orders goes out of scope

// References (no allocation!)
fn process(order: &Order) {  // &Order = reference, no copy
    println!("{}", order.id);
}
```

### Zero-Copy Serialization

```rust
// Instead of: String → parse → Object (2 allocations)
// Rust can parse directly: &[u8] ← reference to bytes

let json_bytes: &[u8] = br#"{"id":"123","price":100}"#;
let order: Order = serde_json::from_slice(json_bytes)?;
// No intermediate String allocation!
```

### Iterators (Lazy Evaluation)

```rust
// TypeScript: Creates intermediate arrays
const filled = data
    .filter(x => x.price > 100)
    .map(x => x * 2)
    .filter(x => x < 1000);
// Creates 3 arrays in memory!

// Rust: Single pass, no intermediate arrays
let filled: Vec<_> = data
    .iter()
    .filter(|x| x.price > 100)
    .map(|x| x * 2)
    .filter(|x| x < 1000)
    .collect();
// Iterator adapters don't allocate until .collect()
// More efficient!
```

---

## **Key Takeaways**

1. **Rust's power:** Type system + Ownership = Impossible data races
2. **Tokio:** Multiplexes thousands of concurrent tasks on thread pool
3. **Three patterns:** Request-response, Pub-Sub, Fire-and-forget
4. **Redis:** Glue connecting async services
5. **Memory safety:** Stack/Heap, Arc/Mutex, References all handled safely

**Next:** Read [`crate-by-crate-guide.md`](./crate-by-crate-guide.md) for specific code walkthroughs.
