# Order Flow Walkthrough: Complete Trace with Code

This document traces a single order through the entire system, showing exactly which code runs when.

---

## **Scenario: User Places a BUY Order**

```
User (Client):  POST http://localhost:8080/api/v1/order
                Body: { "user_id": "alice", "side": "BUY", "price": 100, "qty": 1 }
```

---

## **Step 1: Order Arrives at Router**

**File:** `crates/router/src/main.rs`

**Incoming HTTP Request:**
```
POST /api/v1/order HTTP/1.1
Host: localhost:8080
Content-Type: application/json

{
  "user_id": "alice",
  "side": "BUY",
  "price": 100,
  "qty": 1
}
```

**Code that runs:**

```rust
// crates/router/src/routes/order.rs
#[post("/api/v1/order")]
pub async fn create_order(
    web::Json(payload): web::Json<CreateOrderRequest>,
    state: web::Data<AppState>,
) -> Result<web::Json<OrderResponse>, ApiError> {
    println!("📨 Received order: {:?}", payload);
    
    // 1. Generate unique request ID
    let request_id = uuid::Uuid::new_v4().to_string();
    println!("🔑 Generated request_id: {}", request_id);
    
    // CreateOrderRequest defined at top of file:
    // pub struct CreateOrderRequest {
    //     pub user_id: String,
    //     pub side: Side,      // BUY or SELL
    //     pub price: Decimal,
    //     pub qty: Decimal,
    // }
    
    // 2. Convert request to engine Order
    let order = Order {
        id: request_id.clone(),
        user_id: payload.user_id.clone(),  // "alice"
        side: payload.side,                // Side::BUY
        price: payload.price,              // 100
        qty: payload.qty,                  // 1
        create_time: chrono::Utc::now().timestamp_millis(),
    };
    
    println!("📦 Created order: {:?}", order);
```

**At this point, Router has:**
- ✅ Parsed JSON request
- ✅ Created Order struct
- ✅ Validated types (Rust compiler did this!)

**Next: Contact Redis**

```rust
    // 3. Subscribe to response channel before pushing order
    // This is the KEY to request-response pattern!
    println!("🔔 Subscribing to response channel: {}", request_id);
    
    let mut subscription = state.redis
        .subscribe(&request_id)
        .await
        .map_err(|e| {
            eprintln!("❌ Redis subscribe error: {}", e);
            ApiError::RedisError(e)
        })?;
    
    // If we get here, we're subscribed! Now we'll wait...
```

**Redis Internal State After Subscribe:**
```
Redis:
  SUBSCRIBE request_id (Router is now listening)
  └─ Channel: "550e8400-e29b-41d4-a716-446655440000" (the UUID)
     └─ Subscriber: Router handler
```

**Continue in Router:**

```rust
    // 4. Serialize order to JSON
    let order_json = serde_json::to_string(&order)
        .map_err(|e| ApiError::SerializationError(e))?;
    
    println!("📤 Serialized order: {}", order_json);
    // Output would be:
    // {"id":"550e8400...","user_id":"alice","side":"BUY","price":100,"qty":1,"create_time":1709977200000}
    
    // 5. Push to ORDERS queue
    println!("➡️  Pushing to ORDERS queue");
    state.redis
        .rpush("ORDERS", &order_json)
        .await
        .map_err(|e| {
            eprintln!("❌ Redis rpush error: {}", e);
            ApiError::RedisError(e)
        })?;
    
    // Redis now has:
    // Redis:
    //   ORDERS queue:
    //   └─ [1] { "id": "550e8400...", ... }  ← Order we just pushed
```

**The CRITICAL MOMENT:**

```rust
    // 6. **BLOCKING**: Wait for response from Engine
    println!("⏳ Waiting for Engine response (max 5 seconds)...");
    
    let response_json = tokio::time::timeout(
        Duration::from_secs(5),
        subscription.next(),  // This is ASYNC (will wait)
    )
    .await
    .map_err(|_| {
        eprintln!("❌ Timeout waiting for Engine");
        ApiError::Timeout
    })?;
    
    println!("📥 Received response from Engine: {}", response_json);
    
    // Control flow:
    // - Router handler PAUSES here (doesn't consume CPU)
    // - Tokio pauses this task
    // - Other HTTP requests can be processed
    // - This handler will RESUME when Engine publishes to request_id
```

**Timeline at this point:**
```
T=0ms:    Request arrives → Router spawns handler
T=0.5ms:  Router pushes order to Redis ORDERS queue
T=0.5ms:  Router PAUSES, waiting for response (handler suspended)
          [Tokio is now free to handle other requests]
```

---

## **Step 2: Engine Processes Order**

### **Engine Main Loop**

**File:** `crates/engine/src/main.rs`

```rust
#[tokio::main]
async fn main() {
    // Setup (skipped for brevity)
    let redis = create_redis_client().await;
    let mut engine = Engine::new();
    
    // MAIN LOOP: Process orders
    loop {
        // Pop from ORDERS queue (blocks until message available)
        if let Some(order_json) = redis.lpop("ORDERS").await {
            println!("🎯 Engine: Popped order from queue");
            
            // Deserialize JSON → Order
            let order: Order = serde_json::from_str(&order_json)
                .expect("Failed to parse order");
            
            println!("🎯 Engine: Processing order: {:?}", order);
            
            // Process it (see below)
            match process_order(&mut engine, order).await {
                Ok(fills) => {
                    println!("✅ Engine: Order matched!");
                    // Publish results (see step 3)
                }
                Err(e) => {
                    println!("❌ Engine: Error: {:?}", e);
                }
            }
        }
    }
}
```

**Redis State at this point:**
```
Redis ORDERS queue (before lpop):
  [1] {"id":"550e8400...","user_id":"alice",...}

Engine: lpop("ORDERS")
  ↓
Redis ORDERS queue (after lpop):
  [] (queue empty, message removed)

Engine has: {"id":"550e8400...","user_id":"alice",...}
```

### **Engine Order Matching**

**File:** `crates/engine/src/engine/engine.rs`

```rust
async fn process_order(engine: &mut Engine, order: Order) -> Result<Vec<Fill>> {
    println!("📊 Engine: Starting to match order");
    
    // 1. Validate user exists
    println!("👤 Engine: Checking user balance");
    let user_balance = engine.balances.get(&order.user_id)
        .ok_or_else(|| {
            eprintln!("❌ User not found: {}", order.user_id);
            EngineError::UserNotFound
        })?;
    
    // 2. Lock user's balance
    println!("🔒 Engine: Locking balance for {}", order.user_id);
    let mut balance = user_balance.lock().await;
    // ^^ Now ONLY this task can modify balance
    
    // 3. Check balance
    let required = order.price * order.qty;  // 100 * 1 = 100 USDC
    let available = balance.available.get("USDC").copied().unwrap_or_default();
    
    println!("💰 Engine: Required: {}, Available: {}", required, available);
    
    if available < required {
        return Err(EngineError::InsufficientBalance { required, available });
    }
    
    // 4. Deduct from balance (lock the funds)
    balance.available.insert("USDC", available - required);
    balance.locked.insert("USDC", required);
    println!("💳 Engine: Locked {} USDC for order", required);
    
    // Release balance lock (IMPORTANT: drop it before matching!)
    drop(balance);
    println!("🔓 Engine: Released balance lock");
    
    // 5. Get orderbook and perform matching
    println!("📚 Engine: Getting orderbook");
    let book = &mut engine.orderbooks[0];  // SOL_USDC
    
    let mut fills = Vec::new();
    
    // Our order is a BUY, so match against SELL orders
    if order.side == Side::BUY {
        println!("📈 Engine: Matching BUY order");
        
        // Iterate through ask (sell) orders, lowest price first
        for (ask_price, asks) in book.asks.iter_mut() {
            println!("  📍 Checking ask @ price: {}", ask_price);
            
            if ask_price > &order.price {
                println!("  ❌ Ask price too high ({} > {}), stop matching", ask_price, order.price);
                break;
            }
            
            for ask_order in asks.iter_mut() {
                if ask_order.qty <= Decimal::ZERO {
                    continue;  // Already filled
                }
                
                println!("  ✨ Found matching ask!");
                
                // Create fill
                let fill_qty = order.qty.min(ask_order.qty);
                let fill = Fill {
                    id: uuid::Uuid::new_v4().to_string(),
                    buy_order_id: order.id.clone(),
                    sell_order_id: ask_order.id.clone(),
                    qty: fill_qty,
                    price: *ask_price,
                    timestamp: current_timestamp(),
                };
                
                println!("  💹 Fill: {} SOL @ {} USDC", fill_qty, ask_price);
                
                // Update balances
                // ... (complex balance transfers, omitted for clarity)
                
                fills.push(fill);
            }
        }
        
        // If quantity remains unmatched, add to book as limit order
        if order.qty > Decimal::ZERO {
            println!("📌 Engine: Adding remaining order to book");
            book.bids
                .entry(order.price)
                .or_insert_with(Vec::new)
                .push(order);
        }
    }
    
    println!("🎉 Engine: Matching complete, {} fills", fills.len());
    Ok(fills)
}
```

**Timeline at this point:**
```
T=0ms:      Router pushed order
T=0-5ms:    Engine processing (matching)
T=5ms:      Engine has fills ready to publish
```

---

## **Step 3: Engine Publishes Results (Critical: Multiple Channels!)**

**File:** `crates/engine/src/main.rs`

```rust
// After process_order() returns with fills
let fills = process_order(&mut engine, order.clone()).await?;

println!("📢 Engine: Publishing {} fills", fills.len());

for fill in fills.iter() {
    println!("📤 Engine: Publishing fill: {:?}", fill);
    
    let fill_json = serde_json::to_string(&fill)?;
    
    // ============================================
    // CHANNEL 1: Response to Router (Request-Response)
    // ============================================
    println!("  ➜ Publishing to response channel ({})", order.id);
    
    let response = OrderResponse {
        status: OrderStatus::Filled,
        order_id: order.id.clone(),
        filled_qty: fill.qty,
        // ...
    };
    
    state.redis
        .publish(
            &order.id,  // This is the request_id from Router!
            &serde_json::to_string(&response)?,
        )
        .await?;
    
    println!("  ✅ Published to request response channel");
    
    // ============================================
    // CHANNEL 2: Broadcast To Market Data (Pub/Sub)
    // ============================================
    println!("  ➜ Publishing to market channel (trade.SOL_USDC)");
    
    state.redis
        .publish(
            "trade.SOL_USDC",
            &fill_json,
        )
        .await?;
    
    println!("  ✅ Published to market broadcast channel");
    
    // ============================================
    // CHANNEL 3: Queue for Persistence (Fire-and-Forget)
    // ============================================
    println!("  ➜ Queuing for database persistence");
    
    state.redis
        .rpush("DATABASE_QUEUE", &fill_json)
        .await?;
    
    println!("  ✅ Queued for database");
}
```

**Redis State After Publishing:**
```
Redis After Publishing:

1. REQUEST_ID Channel:
   └─ Subscribers: [Router handler for "550e8400..."]
   └─ Message: {"status":"FILLED","order_id":"550e8400...","filled_qty":1}
      ✅ Router handler WAKES UP here!

2. TRADE.SOL_USDC Channel:
   └─ Subscribers: [WS-Stream handler, any other listeners]
   └─ Message: {"id":"...", "qty":1, "price":100, ...}
      ✅ All subscribed clients get notified

3. DATABASE_QUEUE:
   └─ [1] {"id":"...", "qty":1, "price":100, ...}
      ✅ DB-Processor will consume this
```

**Timeline:**
```
T=5ms:    Engine publishes to request_id channel
          ↓ Router handler RESUMES (was paused since T=0.5ms)
```

---

## **Step 4: Router Receives Response & Sends HTTP Reply**

**File:** `crates/router/src/routes/order.rs` (continuing from Step 1)

```rust
    // ← We were waiting here since T=0.5ms
    // subscription.next() RETURNED with response!
    
    println!("📥 Router: Received response from Engine");
    
    // subscription.next() returned: {"status":"FILLED","order_id":"550e8400...","filled_qty":1}
    let response_json: String = subscription.next().await??;
    
    // 7. Parse response
    println!("🔍 Router: Parsing response JSON");
    let response: OrderResponse = serde_json::from_str(&response_json)
        .map_err(|e| ApiError::ResponseParseError(e))?;
    
    println!("✅ Router: Sending HTTP 200");
    
    // 8. Send HTTP 200 to client
    Ok(web::Json(response))
    
    // HTTP Response sent:
    // 200 OK
    // Content-Type: application/json
    //
    // {
    //   "status": "FILLED",
    //   "order_id": "550e8400-e29b-41d4-a716-446655440000",
    //   "filled_qty": 1
    // }
}
```

**Timeline:**
```
T=0ms:      Request arrives
T=0.5ms:    Router pushes to queue, PAUSES
T=5ms:      Engine processes, publishes response
T=5.1ms:    Router RESUMES
T=5.2ms:    HTTP 200 sent to client
            ← Total latency: 5.2ms ✅
```

**Client receives:**
```json
{
  "status": "FILLED",
  "order_id": "550e8400-e29b-41d4-a716-446655440000",
  "filled_qty": 1,
  "price": 100
}
```

---

## **Step 5: WS-Stream Broadcasts Trade**

**Parallel process (happens at same time as Step 4)**

**File:** `crates/ws-stream/src/ws_manager.rs`

```rust
// WS-Stream is ALWAYS listening to Redis channels
// It was waiting here before the order even arrived:

while let Some(message) = subscription.next().await {
    // "trade.SOL_USDC" channel
    // ↑ Engine just published here!
    
    println!("📻 WS-Stream: Received trade message");
    
    let fill: Fill = serde_json::from_str(&message)?;
    println!("📻 WS-Stream: Got fill: {:?}", fill);
    
    // Broadcast to all connected WebSocket clients
    println!("📡 WS-Stream: Broadcasting to {} subscribers", subscribers.len());
    
    for (client_id, subscribed_channels) in subscribers.iter() {
        if subscribed_channels.contains(&"trade.SOL_USDC".to_string()) {
            println!("  📡 Sending to client: {}", client_id);
            
            // Send WebSocket frame
            client_tx.send(
                ws::Message::Text(
                    serde_json::to_string(&fill)?
                )
            ).await?;
        }
    }
}
```

**Connected WebSocket clients receive:**
```
WebSocket Frame:
{
  "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
  "buy_order_id": "550e8400-e29b-41d4-a716-446655440000",
  "sell_order_id": "bob-order-123",
  "qty": 1,
  "price": 100,
  "timestamp": 1709977205123
}
```

---

## **Step 6: DB-Processor Persists Trade (Background)**

**Async, lower priority (happens in background)**

**File:** `crates/db-processor/src/main.rs`

```rust
// DB-Processor main loop (always running)
loop {
    // Pop from DATABASE_QUEUE
    if let Some(trade_json) = redis.lpop("DATABASE_QUEUE").await {
        println!("🗄️  DB-Processor: Got trade from queue");
        
        let fill: Fill = serde_json::from_str(&trade_json)?;
        println!("🗄️  DB-Processor: Processing fill: {:?}", fill.id);
        
        // Insert into Postgres
        sqlx::query(
            r#"
            INSERT INTO trades (
                id, buy_user_id, sell_user_id, qty, price, timestamp
            ) VALUES ($1, $2, $3, $4, $5, $6)
            "#
        )
        .bind(&fill.id)
        .bind(&fill.buy_order_id)
        .bind(&fill.sell_order_id)
        .bind(&fill.qty)
        .bind(&fill.price)
        .bind(&fill.timestamp)
        .execute(&db_pool)
        .await?;
        
        println!("✅ DB-Processor: Inserted into Postgres");
    }
}
```

**Postgres receives:**
```sql
INSERT INTO trades (id, buy_user_id, sell_user_id, qty, price, timestamp)
VALUES (
  'f47ac10b-58cc-4372-a567-0e02b2c3d479',
  'alice',
  'bob',
  1,
  100,
  1709977205123
);

-- Inserted into:
-- trades table (persistent storage)
```

**Timeline:**
```
T=0ms:      Request arrives
T=5.2ms:    User gets HTTP 200 ✅
T=5.2ms:    WS clients get real-time update ✅
T=10-50ms:  DB-Processor writes to Postgres ✅ (doesn't block trading)
```

---

## **Complete Message Flow Diagram**

```
┌─────────────┐
│   CLIENT    │
│  (browser)  │
└──────┬──────┘
       │
       │ POST /api/v1/order
       │ { user_id: "alice", side: "BUY", price: 100, qty: 1 }
       ▼
┌──────────────────┐
│     ROUTER       │
│  (Port 8080)     │
│                  │
│ 1. Parse JSON    │
│ 2. Gen UUID      │
│ 3. Subscribe     │  ──┐
└──────┬───────────┘    │
       │                │ Request ID: "550e8400-..."
       │ ORDERS queue   │
       ▼                │
    [Redis]            │
    └───┐              │
        │ LPOP         │
        │              │
        ▼              │
   ┌──────────────┐    │
   │    ENGINE    │    │
   │  (Port 8081) │    │
   │              │    │
   │ 1. Lock      │    │
   │ 2. Match     │    │
   │ 3. Create    │    │
   │    fills     │    │
   └──────┬───────┘    │
          │            │
    ┌─────▼─────┬──────┘  (Publish response to request ID)
    │ PUBLISH   │
    │           │
    ├─ trade.SOL_USDC ──────┐
    ├─ DATABASE_QUEUE ──┐   │
    └───────────────────┼─┐ │
                        │ │ │
                  ┌─────┘ │ │
                  │       │ │
                  ▼       ▼ ▼
              ┌────────────────────────┐
              │    WS-STREAM           │
              │  (Port 4000)           │
              │                        │
              │ Broadcast to clients   │
              └────────────────────────┘
              
              ┌─────────────────────────┐
              │   DB-PROCESSOR          │
              │   (Async, fire & forget)│
              │                         │
              │ INSERT INTO trades      │
              └─────────────────────────┘
                       │
                       ▼
              ┌─────────────────────┐
              │     POSTGRES        │
              │  (Persistent)       │
              └─────────────────────┘

HTTP Response (5ms):     ✅ Client: "Order FILLED"
WebSocket Update (5ms):  ✅ Subscribers: Trade broadcast
Database Write (50ms):   ✅ Postgres: Trade recorded
```

---

## **Key Observations**

### 1. **Three Different Latencies**

| Component | Latency | Why |
|-----------|---------|-----|
| HTTP Response | ~5-10ms | Router↔Engine↔Redis (in-memory) |
| WebSocket Update | ~5-10ms | Engine↔WS-Stream↔Redis (in-memory) |
| Database Write | ~50-100ms | Postgres I/O (disk bound) |

**Insight:** Engine doesn't wait for DB. That's why system is fast!

### 2. **Concurrency in Action**

While our order was being matched:
- Other HTTP requests being processed by other Router handlers
- Other WebSocket broadcasts happening
- DB-Processor working on previous trades

All happening simultaneously without blocking each other!

### 3. **Error Handling**

If any `.await` returns `Err`:
- Router: Returns HTTP 400/500
- Engine: Publishes error to response channel
- WS-Stream: Logs and continues
- DB-Processor: Retries or logs

The `?` operator ensures errors aren't silently ignored.

---

## **Try This: Add Logging**

**To understand execution better, modify files:**

```rust
// At top of main.rs files:
use log::{info, debug, error};
use env_logger::Env;

#[tokio::main]
async fn main() {
    // Initialize logging
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    
    info!("Service starting");
    // ... rest of main
}
```

Then run:
```bash
RUST_LOG=debug cargo run
```

You'll see detailed traces of order flow!

---

## **Next: Read the actual source code**

Pick a crate:
1. Open `crates/router/src/routes/order.rs` - Follow the flow
2. Open `crates/engine/src/engine/engine.rs` - Study matching
3. Open `crates/ws-stream/src/ws_manager.rs` - Understand broadcasting

For each file:
- Read line by line
- Identify the `.await` points (where it can pause)
- Trace variable ownership
- Check error handling with `?`

**Questions to ask yourself:**
- "Where does this value come from?"
- "When does this pause?"
- "What if this `.await` fails?"
- "Who owns this data?"

**See also:**
- `patterns-used.md` - Common Rust patterns in codebase
- `rust-for-typescript-developers.md` - Refresh on concepts
