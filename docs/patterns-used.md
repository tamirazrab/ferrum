# Common Patterns & Idioms Used in the Codebase

This guide identifies Rust patterns you'll encounter and explains why they're used.

---

## **1. Macro: `#[derive(...)]` - Auto-Generate Trait Implementations**

### What You'll See
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub side: Side,
    pub price: Decimal,
    pub qty: Decimal,
    pub create_time: i64,
}
```

### What It Does
Instead of manually writing trait implementations, let the compiler generate them automatically:

```rust
// WITHOUT #[derive(Debug)]
impl Debug for Order {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Order")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field("side", &self.side)
            .field("price", &self.price)
            .field("qty", &self.qty)
            .field("create_time", &self.create_time)
            .finish()
    }
}

// WITH #[derive(Debug)] ← One line does all that!
```

### Common Derives in This Project

| Derive | Purpose | Used In |
|--------|---------|---------|
| `Debug` | Print with `println!("{:?}", order)` | All types |
| `Clone` | Copy struct: `order.clone()` | All types (needed for passing around) |
| `Serialize` | Convert to JSON: serde_json::to_string() | All types that go to Redis/HTTP |
| `Deserialize` | Convert from JSON: serde_json::from_str() | All types coming from Redis/HTTP |
| `PartialEq` | Compare: `order1 == order2` | Types (needed for comparisons) |
| `Copy` | Automatic shallow copy (for small types) | Rarely used (conflicts with Drop) |

### TypeScript Equivalent
```typescript
// TypeScript: No automatic serialization
interface Order {
  id: string;
  user_id: string;
  side: "BUY" | "SELL";
  price: number;
  qty: number;
  create_time: number;
}

// Manual: Convert to JSON
const json = JSON.stringify(order);

// Manual: Convert from JSON
const order: Order = JSON.parse(jsonString);
```

**Rust advantage:** Compiler verifies structure at compile time, catches typos before runtime!

---

## **2. Error Handling: `Result<T, E>` with `?` Operator**

### What You'll See
```rust
pub async fn create_order(order: Order) -> Result<OrderResponse, ApiError> {
    let balance = get_user_balance(&order.user_id).await?;  // ← The ?
    let fills = engine.match_order(order).await?;
    Ok(OrderResponse { success: true })
}
```

### What The `?` Does

```rust
// This:
let balance = get_balance(&user_id).await?;

// Expands to:
let balance = match get_balance(&user_id).await {
    Ok(b) => b,
    Err(e) => return Err(e),  // ← Early return on error!
};
```

**Key insight:** Must handle errors - Compiler forces it!

### Compare to TypeScript

```typescript
// TypeScript: Error handling is optional
async function createOrder(order: Order) {
    const balance = await getBalance(order.user_id);  // What if fails? 🤷
    const fills = await engine.matchOrder(order);     // What if fails? 🤷
    return { success: true };
}

// TypeScript: Can ignore errors
try {
    await createOrder(order);
} catch (e) {
    // Oops, what error? Can be anything
}
```

```rust
// Rust: Error handling is explicit
async fn create_order(order: Order) -> Result<OrderResponse, ApiError> {
    let balance = get_balance(&order.user_id).await?;  // Must define which error type
    let fills = engine.match_order(order).await?;
    Ok(OrderResponse { success: true })
}

// Rust: Must handle each error
match create_order(order).await {
    Ok(response) => println!("Success"),
    Err(ApiError::InsufficientBalance) => println!("Not enough balance"),
    Err(ApiError::InvalidPrice) => println!("Invalid price"),
    // Compiler forces handling all cases!
}
```

### Custom Error Enum
```rust
#[derive(Debug)]
pub enum ApiError {
    RedisError(String),
    InsufficientBalance { required: u64, available: u64 },
    UserNotFound,
    InvalidOrderPrice(String),
    Timeout,
    DatabaseError(sqlx::Error),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RedisError(msg) => write!(f, "Redis error: {}", msg),
            Self::InsufficientBalance { required, available } => {
                write!(f, "Insufficient balance: need {}, have {}", required, available)
            }
            Self::UserNotFound => write!(f, "User not found"),
            // ...
        }
    }
}
```

**Benefit:** Stack trace tells exactly what went wrong and where!

---

## **3. Pattern Matching: `match` Expression**

### Simple Matching
```rust
// TypeScript switch:
switch (order.side) {
    case "BUY":
        // ...
        break;
    case "SELL":
        // ...
        break;
}

// Rust match:
match order.side {
    Side::BUY => {
        // No break needed!
    }
    Side::SELL => {
        // No break needed!
    }
    // COMPILER ERROR if you forget a case!
}
```

### Destructuring with Match
```rust
// Extract fields while matching
match order {
    Order { side: Side::BUY, price, qty, .. } => {
        // Can directly use price and qty!
        println!("Buy order for {} qty @ {}", qty, price);
    }
    Order { side: Side::SELL, .. } => {
        println!("Sell order");
    }
}

// The .. means "ignore other fields"
```

### Matching Result
```rust
// Instead of ?
let balance = get_balance(user_id).await?;

// Explicit handling:
match get_balance(user_id).await {
    Ok(balance) => {
        println!("Balance: {}", balance);
    }
    Err(e) => {
        eprintln!("Error: {}", e);
        return;
    }
}
```

### Matching Option (nullable)
```rust
// In TypeScript: Optional chaining
const user = users.find(u => u.id === id);
if (user) {
    console.log(user.name);
}

// In Rust:
match users.iter().find(|u| u.id == id) {
    Some(user) => println!("Found: {}", user.name),
    None => println!("User not found"),
}

// Shorthand:
if let Some(user) = users.iter().find(|u| u.id == id) {
    println!("Found: {}", user.name);
}
```

**Why:** Exhaustive - compiler forces you to handle all cases!

---

## **4. Ownership Transfer: Move Semantics**

### Basic Move
```rust
let order = Order { id: "123".to_string(), ... };
let order2 = order;  // Move ownership (not copy!)

println!("{:?}", order);   // ❌ ERROR: order no longer owns it
println!("{:?}", order2);  // ✅ OK
```

### With Functions
```rust
// Passing by value = move ownership
async fn process_order(order: Order) {
    // Function now owns order
    println!("{:?}", order);
} // order is freed here

let order = Order { ... };
process_order(order);  // Ownership transferred
// println!("{:?}", order);  // ❌ ERROR: order was moved!
```

### Borrowing to Avoid Move
```rust
// Passing reference = borrow (don't move)
async fn process_order(order: &Order) {
    // Function borrows order
    println!("{:?}", order);
} // Reference ends, order still belongs to caller

let order = Order { ... };
process_order(&order);  // Borrow reference
println!("{:?}", order);  // ✅ OK: still owned by us
```

**In exchange code:**
```rust
// Engine takes ownership
engine.create_order(order).await?;

// Router borrows state
fn handler(state: web::Data<AppState>) {
    // Use &state
}
```

---

## **5. Lifetimes: Indicating Scope of References**

### Simple Lifetime
```rust
// Lifetime 'a = "this reference is valid as long as the input is"
fn get_first<'a>(items: &'a Vec<i32>) -> &'a i32 {
    &items[0]
}

let items = vec![1, 2, 3];
let first = get_first(&items);
println!("{}", first);  // ✅ OK

// Scope ends:
} // items freed, first would be invalid

// But Rust prevents:
let first;
{
    let items = vec![1, 2, 3];
    first = get_first(&items);  // ❌ ERROR: first points to freed items
} // items freed here!
println!("{}", first);  // ❌ ERROR
```

**"Borrow checker says no"** - Rust prevents use-after-free bugs!

### Lifetime Elision (Compiler Infers)
```rust
// Often you don't write lifetimes - compiler infers:
fn get_first(items: &Vec<i32>) -> &i32 {
    &items[0]
}
// Compiler infers: &'a Vec -> &'a i32
```

### In Real Code
```rust
// Router state: web::Data<AppState>
// web::Data has lifetime 'static (exists for entire program)

pub struct AppState {
    pub redis: RedisManager,     // Lifetime: 'static
    pub db_pool: PgPool,         // Lifetime: 'static
}

// So accessing it is safe:
pub async fn handler(state: web::Data<AppState>) {
    state.redis.lpop(...).await  // ✅ Safe: state lives long enough
}
```

---

## **6. Traits: Define Behavior**

### Simple Trait
```rust
// TypeScript interface:
interface Persistable {
    save(): Promise<void>;
}

// Rust trait:
pub trait Persistable {
    async fn save(&self) -> Result<()>;
}

// Implement for Order:
#[async_trait::async_trait]
impl Persistable for Order {
    async fn save(&self) -> Result<()> {
        db.insert_order(self).await
    }
}
```

### Trait Bounds: Generics with Constraints
```rust
// TypeScript: Any type works
function serialize<T>(value: T): string {
    JSON.stringify(value);  // Works for anything
}

// Rust: Only types implementing Serialize trait
fn serialize<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value)
}

// Error example:
struct CustomType { /* ... */ }
serialize(&CustomType { ... });  // ❌ ERROR: CustomType doesn't implement Serialize
// Fix: Add #[derive(Serialize)] or implement manually

impl Serialize for CustomType { /* ... */ }
serialize(&CustomType { ... });  // ✅ Now works
```

### Common Traits in Exchange
| Trait | Purpose | Example |
|-------|---------|---------|
| `Serialize` | Convert to JSON | Order, Fill |
| `Deserialize` | Parse from JSON | Order, Fill |
| `Clone` | Create copies | Used when passing to threads |
| `Debug` | Print for debugging | println!("{:?}", order) |
| `Display` | Nice formatting | println!("{}", error) |
| `Error` | Implement error type | ApiError implements Error |

---

## **7. Closures and `move`**

### Basic Closure
```rust
// TypeScript arrow function:
const add = (a, b) => a + b;
add(1, 2);  // 3

// Rust closure:
let add = |a, b| a + b;
add(1, 2);  // 3
```

### Closure with Move
```rust
// TypeScript: References automatically captured
let names = ["Alice", "Bob"];
const print_names = () => {
    console.log(names);  // Can access names
};

// Rust: Must be explicit
let names = vec!["Alice".to_string(), "Bob".to_string()];
let print_names = || {
    println!("{:?}", names);  // Borrows names
};
print_names();  // ✅ OK
// println!("{:?}", names);  // ✅ OK: names still owned

// With move:
let names = vec!["Alice".to_string(), "Bob".to_string()];
let print_names = move || {
    println!("{:?}", names);  // Takes ownership
};
print_names();  // ✅ OK
// println!("{:?}", names);  // ❌ ERROR: names moved into closure
```

### In Exchange Code
```rust
// Spawn background task listening to Redis
let addr = ctx.address();
tokio::spawn(async move {  // ← move keyword
    let mut subscription = redis.subscribe(&cmd.channel).await?;
    while let Some(msg) = subscription.next().await {
        // Send to WebSocket (addr is moved into this closure)
        addr.do_send(WsMessage(msg));
    }
});
```

**Why move?** The closure outlives the current function scope, so it must own everything it uses.

---

## **8. Option and Result Combinators**

### Option Methods
```rust
// Instead of match, use fluent API:
let balance = get_balance(user_id)
    .map(|b| b * 2)        // If Some, multiply
    .unwrap_or(0);         // If None, use 0

// Chaining:
let discount = user_type
    .and_then(|t| get_discount_for_type(t))  // If Some, call function
    .unwrap_or(0);
```

### Result Methods
```rust
// Chain error handling:
let order = parse_order(json)
    .map_err(|e| ApiError::ParseError(e))?  // Convert error type
    .clone();

// Multiple operations:
let response = create_order(order)
    .and_then(|o| confirm_order(&o))
    .and_then(|o| broadcast_order(&o))
    .map_err(|e| {
        eprintln!("Failed: {}", e);
        e
    })?;
```

---

## **9. Iterator Pattern**

### Iterating Collections
```rust
// TypeScript:
const prices = orders.map(o => o.price);
const expensive = prices.filter(p => p > 100);
expensive.forEach(p => console.log(p));

// Rust (same, but type-safe):
let prices: Vec<_> = orders
    .iter()                    // Get iterator
    .map(|o| o.price)         // Transform each
    .filter(|p| p > 100)      // Filter
    .collect();               // Collect into Vec

// Or without collecting:
for price in prices.iter().filter(|p| p > 100) {
    println!("{}", price);
}
```

### Mutable Iteration
```rust
// Rust: Modify while iterating
for order in orders.iter_mut() {
    order.qty -= 1;  // Modify each
}

// vs TypeScript: Create new array
const reduced = orders.map(o => ({...o, qty: o.qty - 1}));
```

### BTreeMap Iteration (Used in Orderbook)
```rust
// Iterate sorted map (prices from lowest to highest)
for (price, orders) in book.asks.iter() {
    println!("Asks @ {}: {} orders", price, orders.len());
}

// Mutable iteration:
for (price, orders) in book.asks.iter_mut() {
    orders.retain(|o| o.qty > 0);  // Remove filled orders
}

// Get entries, sorted by key:
let mut asks: Vec<_> = book.asks.iter().collect();
asks.sort_by_key(|&(price, _)| price);
```

---

## **10. Async/Await Patterns**

### Simple Await
```rust
// TypeScript:
const user = await fetchUser(id);

// Rust:
let user = fetch_user(id).await;  // Similar!
```

### Concurrent Tasks with `join!` or `select!`
```rust
// TypeScript: Promise.all (concurrent)
const [user, orders] = await Promise.all([
    fetchUser(id),
    fetchOrders(id),
]);

// Rust: tokio::join! (concurrent)
let (user, orders) = tokio::join!(
    fetch_user(id),
    fetch_orders(id),
);
// Both run simultaneously!
```

### Spawning Background Task
```rust
// Keep running without blocking
let handle = tokio::spawn(async {
    loop {
        do_something().await;
    }
});

// Later: wait for completion
handle.await?;
```

### Timeout
```rust
// TypeScript: Promise.race
const result = await Promise.race([
    fetchData(),
    delay(5000).then(() => null),
]);

// Rust: tokio::time::timeout
let result = tokio::time::timeout(
    Duration::from_secs(5),
    fetch_data(),
).await;

match result {
    Ok(data) => println!("Success: {:?}", data),
    Err(_) => println!("Timeout!"),
}
```

---

## **11. Arc and Mutex: Shared Mutable State**

### Understanding Arc
```rust
// TypeScript: Implicit reference counting
const data = [1, 2, 3];
const ref1 = data;
const ref2 = data;
// Garbage collector tracks references

// Rust: Explicit reference counting
use std::sync::Arc;

let data = Arc::new(vec![1, 2, 3]);
let ref1 = Arc::clone(&data);
let ref2 = Arc::clone(&data);
// Arc::clone increments refcount
// When all refs dropped, vector freed
```

### Arc + Mutex for Concurrent Access
```rust
// Shared mutable state across tasks
let counter = Arc::new(Mutex::new(0_i32));

// Task 1
let c1 = Arc::clone(&counter);
tokio::spawn(async move {
    let mut num = c1.lock().await;
    *num += 1;
    // Lock released when num goes out of scope
});

// Task 2
let c2 = Arc::clone(&counter);
tokio::spawn(async move {
    let mut num = c2.lock().await;
    *num += 1;
    // No data race! Only one task holds lock at a time
});
```

---

## **12. Builder Pattern**

### Common in Configuration
```rust
// Actix HttpServer setup:
HttpServer::new(move || {
    App::new()
        .app_data(state.clone())
        .configure(routes)
        .wrap(middleware::Logger::default())
})
.bind("0.0.0.0:8080")?
.run()
.await;

// Each method returns Self, enabling chaining
// Rust compiler ensures correct order
```

---

## **Quick Reference: Recognizing Patterns**

| Pattern | Syntax | Purpose |
|---------|--------|---------|
| **Error Propagation** | `?.await` | Return early on error |
| **Pattern Matching** | `match x { ... }` | Exhaustively handle cases |
| **Conditional Binding** | `if let Ok(x) = y` | Optional pattern matching |
| **Immutable Borrow** | `&value` | Read without transfer |
| **Mutable Borrow** | `&mut value` | Exclusive write access |
| **Move** | `move \|\| { }` | Transfer ownership into closure |
| **Trait Bound** | `<T: Serialize>` | Generic type constraint |
| **Derive Macro** | `#[derive(Debug)]` | Auto-generate impl |
| **Builder** | `.method().method()` | Fluent configuration |
| **Iterator Adapter** | `.map().filter()` | Lazy transformation |
| **Async Task** | `tokio::spawn(async {})` | Background task |

---

## **Next: Reading Code with Understanding**

When you see code like:

```rust
pub async fn handle_order(
    web::Json(req): web::Json<CreateOrderRequest>,
    state: web::Data<AppState>,
) -> Result<web::Json<OrderResponse>, ApiError> {
    let order_id = uuid::Uuid::new_v4().to_string();
    
    let subscription = state.redis
        .subscribe(&order_id)
        .await?;
    
    let order = Order { /* ... */ };
    state.redis.rpush("ORDERS", &order).await?;
    
    let response_json = tokio::time::timeout(
        Duration::from_secs(5),
        subscription.next(),
    )
    .await??;
    
    let response: OrderResponse = serde_json::from_str(&response_json)?;
    Ok(web::Json(response))
}
```

Read it as:

1. ✅ **Async function** - can use `.await`
2. ✅ **Error handling** - returns `Result<..., ApiError>`
3. ✅ **Pattern matching** - extract JSON body
4. ✅ **Borrow checkerand lifetimes** - immutable references (`&`)
5. ✅ **Error propagation** - `?` returns early on error
6. ✅ **Resource management** - subscription freed after timeout
7. ✅ **Type safety** - JSON parsing verified at compile time

---

**You're now ready to read the actual codebase confidently!**

See [`order-flow-walkthrough.md`](./order-flow-walkthrough.md) to see this pattern in action.
