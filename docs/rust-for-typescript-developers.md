# Rust for TypeScript Developers: A Practical Guide

This guide explains core Rust concepts by comparing them directly to TypeScript patterns you already know.

---

## **1. Ownership & Borrowing (The Big One!)**

### The Problem Rust Solves
In TypeScript, everything is a reference:
```typescript
const data = [1, 2, 3];
const ref1 = data;
const ref2 = data;
// All three variables point to same array
// Who "owns" it? The garbage collector will clean it up eventually
```

Rust forces you to be explicit about ownership:

### Ownership Rules
```rust
// 1. Each value has ONE owner
let data = vec![1, 2, 3];  // data OWNS this vector

// 2. When owner goes out of scope, value is deallocated
{
    let data = vec![1, 2, 3];
} // Vector is freed here ← Memory is known at compile time!

// 3. You can MOVE ownership
let data = vec![1, 2, 3];
let new_owner = data;      // Ownership MOVED to new_owner
// println!("{:?}", data); // ❌ ERROR: data no longer owns it!
println!("{:?}", new_owner); // ✅ OK
```

**Why this matters for trading:**
- Memory leaks impossible (critical for long-running services)
- No garbage collection pauses (ultra-low latency order execution)
- Multi-threaded code can't have data races (thread-safe by default)

---

### Borrowing (References)
Instead of moving ownership, you can **borrow** temporarily:

```rust
// TypeScript (implicit references everywhere):
function printArray(arr: number[]) {
    console.log(arr);
}
const data = [1, 2, 3];
printArray(data);  // arr "borrows" data
printArray(data);  // Can call again, data still owned by caller
```

```rust
// Rust (explicit borrowing):
fn print_vector(vec: &Vec<i32>) {
    println!("{:?}", vec);
}

let data = vec![1, 2, 3];
print_vector(&data);  // Pass REFERENCE (borrow)
print_vector(&data);  // Can borrow again, data still owned
```

**The & symbol means "borrow = temporary permission to read"**

### Mutable vs Immutable References
```rust
let mut data = vec![1, 2, 3];

// IMMUTABLE borrow (read-only)
let reader1 = &data;
let reader2 = &data;
println!("{:?}", reader1);  // ✅ Can have many readers
println!("{:?}", reader2);  // ✅ Simultaneous reads OK

// MUTABLE borrow (exclusive access)
let writer = &mut data;
writer.push(4);
// println!("{:?}", reader1);  // ❌ ERROR: Can't read while someone is mutating!
// ✅ XOR rule: Either multiple readers OR one writer, never both
```

**Why this matters:**
- Data race prevention is **built into the language**
- TypeScript: You must manually ensure thread-safety
- Rust: Compiler forces it

---

## **2. Async/Await (Tokio vs Node.js)**

### TypeScript/Node.js Async
```typescript
// Promise-based
async function getUser(id: string): Promise<User> {
    const response = await fetch(`/api/users/${id}`);
    return response.json();
}

// Event loop runs all async operations cooperatively
// If one await takes 100ms, others are still being processed
// This is multiplexing at the library level
```

### Rust/Tokio Async
```rust
// Future-based (similar concept, different semantics)
async fn get_user(id: &str) -> User {
    let response = fetch(format!("/api/users/{}", id)).await;
    response.json().await
}

// Tokio runs on executor that polls all Futures
// Futures are lazy - they don't run until polled
// Tokio runtime spawns tasks that are polled concurrently
```

**Practical difference:**
```typescript
// TypeScript: Implicit event loop
await Promise.all([
    fetch('/api/users/1'),
    fetch('/api/users/2'),
    fetch('/api/users/3'),
]);
// All 3 requests run concurrently (multiplexed by Event Loop)
```

```rust
// Rust: Explicit async runtime (Tokio)
tokio::join!(
    fetch_user(1),
    fetch_user(2),
    fetch_user(3),
);
// All 3 futures are polled concurrently by Tokio executor
// Similar semantics: all run "at the same time"
// But Rust knows at compile-time which are async (more optimizations)
```

### Key Difference: Futures vs Promises
```typescript
// TypeScript: Promise starts executing immediately
const promise = fetch('/api/users/1');  // Request already sent!
await promise;
```

```rust
// Rust: Future is lazy - doesn't execute until polled
let future = fetch_user(1);  // Nothing happens yet
tokio::spawn(future);        // Now it runs in background
```

**For exchange: Tokio lets us spawn thousands of concurrent orders with minimal overhead**

---

## **3. Result<T, E> Error Handling**

### TypeScript Error Pattern
```typescript
function parseOrder(json: string): Order {
    try {
        return JSON.parse(json);
    } catch (e) {
        throw new Error(`Invalid order: ${e.message}`);
    }
}

// Problem: Error handling is optional (can ignore it)
const order = parseOrder(userInput);  // What if it fails? 🤷
```

### Rust Error Pattern
```rust
fn parse_order(json: &str) -> Result<Order, ParseError> {
    serde_json::from_str(json)
        .map_err(|e| ParseError::InvalidJson(e.to_string()))
}

// Usage: MUST handle the Result
let order = parse_order(user_input)?;  // ? = early return on error
// OR
match parse_order(user_input) {
    Ok(order) => println!("Order: {:?}", order),
    Err(e) => eprintln!("Error: {}", e),
}
```

**Why this matters:**
- TypeScript: Unhandled errors can crash production
- Rust: Compiler forces you to handle errors explicitly
- `?` operator = shorthand for "if error, return early"

### Custom Error Types
```rust
#[derive(Debug)]
pub enum EngineError {
    InsufficientBalance { required: u64, available: u64 },
    InvalidOrderPrice(String),
    MarketNotFound(String),
}

// Usage with context
fn place_order(user_id: &str, amount: u64) -> Result<Order, EngineError> {
    let balance = get_balance(user_id);
    if balance < amount {
        return Err(EngineError::InsufficientBalance {
            required: amount,
            available: balance,
        });
    }
    Ok(create_order(user_id, amount))
}
```

---

## **4. Traits (Interfaces on Steroids)**

### TypeScript Interfaces
```typescript
interface User {
    id: string;
    name: string;
    getBalance(): Promise<number>;
}

class TradingUser implements User {
    id: string;
    name: string;
    async getBalance() {
        return await db.query(`SELECT balance FROM users WHERE id = ?`, this.id);
    }
}
```

### Rust Traits
```rust
pub trait User {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    async fn get_balance(&self) -> Result<u64, Error>;
}

pub struct TradingUser {
    id: String,
    name: String,
}

impl User for TradingUser {
    fn id(&self) -> &str { &self.id }
    fn name(&self) -> &str { &self.name }
    async fn get_balance(&self) -> Result<u64, Error> {
        db.query_balance(&self.id).await
    }
}
```

**Key difference: Rust traits are more flexible**
```rust
// Generic function that works for ANY type implementing User trait
fn print_user_info<T: User>(user: &T) {
    println!("User: {} ({})", user.name(), user.id());
}

// Works for TradingUser, AdminUser, BotUser, etc.
```

---

## **5. Pattern Matching (The Most Powerful Feature)**

### TypeScript Switch
```typescript
function handleOrderStatus(status: string) {
    switch (status) {
        case "PENDING":
            console.log("Waiting for match...");
            break;
        case "FILLED":
            console.log("Order executed!");
            break;
        case "CANCELLED":
            console.log("Order cancelled");
            break;
        default:
            console.log("Unknown status");
    }
}
```

### Rust Pattern Matching
```rust
fn handle_order_status(status: OrderStatus) {
    match status {
        OrderStatus::Pending => println!("Waiting for match..."),
        OrderStatus::Filled(price, amount) => {
            println!("Order executed at {} for {}", price, amount)
        }
        OrderStatus::Cancelled { reason } => {
            println!("Order cancelled: {}", reason)
        }
        // ❌ ERROR if you forget a case! Compiler forces exhaustiveness
    }
}
```

**Pattern matching with Options (like nullable values):**
```rust
// Rust: Option = "value might not exist"
fn get_user_balance(user_id: &str) -> Option<u64> {
    // Returns Some(1000) or None
}

// Must handle both cases
let balance = get_user_balance("user1");
match balance {
    Some(amount) => println!("Balance: {}", amount),
    None => println!("User not found"),
}

// Shorthand: unwrap_or with default
let amount = get_user_balance("user1").unwrap_or(0);  // 0 if not found
```

**In exchange code:** Match on order status, trade type, error types - compiler ensures all cases handled!

---

## **6. Generics & Type Parameters**

### TypeScript Generics
```typescript
function wrapInArray<T>(value: T): T[] {
    return [value];
}

const numbers = wrapInArray(42);       // T = number
const strings = wrapInArray("hello");  // T = string
```

### Rust Generics (Same Concept)
```rust
fn wrap_in_vec<T>(value: T) -> Vec<T> {
    vec![value]
}

let numbers = wrap_in_vec(42);       // T = i32
let strings = wrap_in_vec("hello");  // T = &str
```

**With trait bounds (constraints):**
```rust
// Works only if T can be printed
fn print_value<T: std::fmt::Display>(value: T) {
    println!("{}", value);
}

print_value(42);        // ✅ i32 implements Display
print_value("hello");   // ✅ &str implements Display
// print_value(vec![1,2,3]);  // ❌ Vec doesn't implement Display
```

---

## **7. Macros (Code Generation)**

### TypeScript Decorators (similar concept)
```typescript
@LogExecution
@ValidateInput
async function placeOrder(order: Order) {
    // Code runs with logging + validation wrapped
}
```

### Rust Macros (more powerful)
```rust
#[derive(Debug, Serialize, Deserialize)]  // Auto-generate code
pub struct Order {
    pub id: String,
    pub price: u64,
    pub qty: u64,
}

// Expands to ~50 lines of impl blocks automatically!
// Prevents boilerplate code

// Macro invocation (looks like function but generates code)
vec![1, 2, 3]  // This is a MACRO, not a function!
```

**Key macros in exchange:**
- `#[derive(...)]` - Auto-generate trait implementations
- `serde_json::json!` - Create JSON without string parsing
- `tokio::spawn` - Create async task (compile-time optimized)

---

## **8. Lifetimes (Rust's Unique Feature)**

### The Problem
```rust
fn get_first_element(vec: &Vec<i32>) -> &i32 {
    &vec[0]
}

// Question: How long is the returned reference valid?
// Answer: As long as the vector exists
```

### Lifetime Annotations
```rust
// Rust can usually infer, but explicit:
fn get_first_element<'a>(vec: &'a Vec<i32>) -> &'a i32 {
    &vec[0]
}

// 'a = lifetime parameter = "the reference is valid as long as the input vector"
```

**Why it matters:**
```rust
let result;
{
    let vec = vec![1, 2, 3];
    result = get_first_element(&vec);  // result borrows from vec
} // vec is freed here!

// println!("{}", result);  // ❌ ERROR: result points to freed memory
// Rust catches this at compile-time!
```

**In TypeScript, this would be a use-after-free bug at runtime**

---

## **9. Concurrency Primitives**

### TypeScript: Multiple Approaches
```typescript
// Callbacks
setTimeout(() => {}, 1000);

// Promises
Promise.resolve(1).then();

// Async/await
await fetchData();

// EventEmitter
emitter.on('event', () => {});

// Libraries abstract it
const channel = new Channel();
```

### Rust: Consistent Abstractions
```rust
// Tokio tasks (== JavaScript promises)
tokio::spawn(async { /* runs in background */ });

// Channels (message passing)
let (tx, mut rx) = tokio::sync::mpsc::channel(100);
tokio::spawn(async move {
    tx.send(value).await;  // Send message
});

// Mutex (exclusive access)
let data = std::sync::Arc::new(Mutex::new(vec![1, 2, 3]));
let mut locked = data.lock().unwrap();
locked.push(4);  // Exclusive access while locked

// Atomic (for simple values)
let counter = std::sync::atomic::AtomicU64::new(0);
counter.fetch_add(1, Ordering::SeqCst);  // Thread-safe increment
```

**In exchange: Use channels for message passing, Mutex for shared state**

---

## **10. Memory Layout & Performance**

### Stack vs Heap
```typescript
// TypeScript: Everything complex is heap-allocated
const user: User = {
    id: "123",
    balance: 1000000,
};
// user is on HEAP, variable holds POINTER
```

```rust
// Rust: Stack by default, you choose Heap
struct User {
    id: String,      // String is HEAP-allocated (like Vec)
    balance: u64,    // u64 is STACK-allocated (like number)
}

let user = User {
    id: "123".to_string(),
    balance: 1_000_000,
};
// Stack: pointer to heap (id), + u64 (balance)
// ✅ Same memory as TypeScript
// But Rust knows the size at compile-time (more optimizations)
```

**For ultra-low latency trading:**
- Use stack-allocated values when possible (faster)
- Small string literals: `"static"` (no allocation)
- Use `String` only when needed (dynamic allocation)

---

## **11. Rust's Scary Error Messages (They're Actually Helpful!)**

### Example: Borrow Checker
```rust
let mut data = vec![1, 2, 3];
let ref1 = &data;        // Immutable borrow
data.push(4);            // ❌ ERROR: Can't mutate while borrowed!

error: cannot borrow `data` as mutable because it is also borrowed as immutable
 --> main.rs:3:5
  |
2 |     let ref1 = &data;
  |                ----- immutable borrow occurs here
3 |     data.push(4);
  |     ^^^^^^^^^^^ mutable borrow occurs here
4 |
5 |     println!("{:?}", ref1);
  |                      ---- immutable borrow later used here

help: consider moving the immutable borrow earlier
```

**This looks scary but it's telling you exactly what's wrong!**
- Other languages let you mutate while someone is reading → data corruption
- Rust prevents it before runtime

---

## **Summary Table: TS vs Rust Concepts**

| TypeScript | Rust | Key Difference |
|---|---|---|
| `const x = 5` | `let x = 5;` | Immutable by default (both) |
| `let x: number` | `let x: i32` | Rust is explicit, types required |
| `x instanceof User` | `match x { User => }` | Pattern matching is more powerful |
| `async function` | `async fn` | Same semantics, Rust is zero-cost |
| `Promise.resolve()` | `async {}` | Promises run immediately, Futures are lazy |
| `throw new Error()` | `Err(e)?` | Rust forces error handling |
| `interface User {}` | `trait User {}` | Traits are more flexible (no inheritance) |
| `null / undefined` | `Option<T>` | Rust forces null-safety at compile-time |
| `any` | `dyn Trait` | Rust still type-safe (Any is bad) |
| Garbage collection | RAII + Ownership | Rust: deterministic cleanup |
| Data races: possible | Data races: impossible | Rust prevents at compile-time |

---

## **Next Steps: Reading the Exchange Codebase**

Now that you understand these concepts, when you see:

```rust
pub async fn create_order(
    state: &mut Engine,
    order: &Order,
) -> Result<Fill, EngineError> {
```

You should read it as:
- `pub` = public function
- `async` = can be awaited, returns Future
- `state: &mut Engine` = borrow Engine mutably (only this function can modify it)
- `order: &Order` = borrow Order immutably (won't modify it)
- `Result<Fill, EngineError>` = returns either Fill or EngineError, must handle
- Returns Future that resolves to the Result

**This is the foundation. Next docs will walk through actual code!**
