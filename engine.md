# engine.rs — Complete Deep Dive

> The matching engine, balance ledger, and message bus coordinator for the crypto exchange. Single-threaded, Redis-driven, in-memory.

---

## Table of Contents

1. [The Layman Mental Model](#1-the-layman-mental-model)
2. [Architecture Overview](#2-architecture-overview)
3. [Data Structures](#3-data-structures)
4. [Complete Data Flow](#4-complete-data-flow)
5. [Every Function Explained](#5-every-function-explained)
6. [Rust Concepts Used](#6-rust-concepts-used)
7. [Where You Will Get Stuck](#7-where-you-will-get-stuck)
8. [Build Plan — 100 Steps](#8-build-plan--100-steps)
9. [Mental Model Cheatsheet](#9-mental-model-cheatsheet)

---

## 1. The Layman Mental Model

Before any Rust. Before any code. Read this.

### What is engine.rs in plain English?

Imagine a stock exchange trading floor — but for crypto. There are two giant whiteboards:

- **Whiteboard 1 (Bids):** People who want to BUY SOL and the max price they'll pay
- **Whiteboard 2 (Asks):** People who want to SELL SOL and the min price they'll accept

One person stands between these whiteboards. Their job:

1. Accept new orders from traders (via a Redis ticket queue)
2. Check if any order on the opposite whiteboard matches the new order
3. If match → execute the trade, update both traders' bank accounts
4. If no match → write the order on the board and wait
5. Broadcast the result so everyone watching a screen sees live updates

**`engine.rs` IS that person.** It owns the whiteboards (`orderbooks`) and the bank vault (`balances`).

### The One-Line Summary

```
Redis queue → Engine::process() → orderbook.add_order() → fills → update_balance() → broadcast to Redis pub/sub
```

The engine never touches HTTP. It only speaks Redis.

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         SYSTEM OVERVIEW                         │
├─────────────────┬───────────────────┬───────────────────────────┤
│   Axum REST API │   Engine (you)    │   DB Processor Service    │
│                 │                   │                           │
│  HTTP Request   │  BRPOP from Redis │  BRPOP from DB queue      │
│      ↓          │       ↓           │       ↓                   │
│  LPUSH to Redis │  process()        │  Write to PostgreSQL      │
│      ↑          │       ↓           │                           │
│  BRPOP response │  match orders     │   WebSocket Server        │
│                 │       ↓           │                           │
│                 │  LPUSH response   │  SUB to Redis pub/sub     │
│                 │  LPUSH DB queue   │  Broadcast to clients     │
│                 │  PUB to WS channel│                           │
└─────────────────┴───────────────────┴───────────────────────────┘
```

### Why Single-Threaded?

Trading engines are deliberately single-threaded. Every order must see exactly the state left by all previous orders. Multi-threading requires complex locking around the entire book — deadlock risk, latency spikes, and non-deterministic execution order. Single thread + async Redis I/O = simple, correct, fast enough for a portfolio exchange.

### What Owns What

```
Engine {
    orderbooks: Vec<Orderbook>         ← source of truth for all open orders
    balances: HashMap<uid, HashMap<currency, UserBalance>>  ← source of truth for all funds
}
```

Nothing else holds state. Redis is just message transport. Postgres is eventual persistence.

---

## 3. Data Structures

### `UserBalance`

```rust
pub struct UserBalance {
    available: f64,   // money the user can spend RIGHT NOW
    locked: f64,      // money held in escrow for open orders
}
```

**Real-world analogy:** Think of your bank account. `available` is your checking balance. `locked` is a hold your bank puts on funds when you write a check — you can see it, but you can't spend it until the check clears or bounces.

When a user places a buy order:
- `available -= price * quantity` (funds reserved)
- `locked += price * quantity` (funds in escrow)

When the order fills:
- `locked -= fill amount` (escrow released, transferred to seller)

When the order is cancelled:
- `locked -= remaining` → `available += remaining` (funds returned)

### `Engine`

```rust
pub struct Engine {
    pub orderbooks: Vec<Orderbook>,
    balances: HashMap<String, HashMap<String, UserBalance>>,
}
```

The `balances` field is a nested HashMap. Visualize it as a spreadsheet:

```
            SOL           USDC          INR
user_abc  {avail:5, lock:0}  {avail:1000, lock:200}  {avail:50000, lock:0}
user_xyz  {avail:0, lock:2}  {avail:800,  lock:0  }  {avail:10000, lock:0}
```

Access pattern: `self.balances["user_abc"]["USDC"].available`

---

## 4. Complete Data Flow

### Happy Path: Buy Order Fully Matched

Scenario: User2 has 5 SOL sitting in the ask book at $100. User1 clicks "Buy 5 SOL at $100".

```
1.  User1's browser  →  POST /order  →  Axum API
2.  Axum API         →  LPUSH "engine_queue" {CreateOrder, market: SOL_USDC, price: 100, qty: 5, side: Buy}
3.  Engine loop      →  BRPOP "engine_queue"  (wakes up)
4.  process()        →  routes to CreateOrder arm
5.  create_order()   →  ensure_user_balance("user1")
6.                   →  find SOL_USDC orderbook
7.                   →  build Order struct
8.  add_order()      →  calls match_bid()
9.  match_bid()      →  finds User2's ask at $100, fills 5 SOL
10.                  →  returns ([Fill{qty:5, price:100, other_user:"user2"}], executed_qty:5.0)
11. executed_qty == order.quantity  →  order NOT added to bids (fully filled)
12. update_balance() →  User2 gains USDC, User1 gains SOL
13. create_db_trades() → push TradeMessage to "db_processor" Redis queue
14. update_db_orders() → push OrderUpdate for both orders to "db_processor"
15. publish_ws_depth_updates() → PUB to "depth@SOL_USDC" channel (ask at $100 now empty)
16. publish_ws_trades()        → PUB to "trade@SOL_USDC" channel
17. return Ok((5.0, fills, order_id))
18. process()        →  LPUSH response to "api_response_{client_id}"
19. Axum API         →  BRPOP response  →  return HTTP 200 to User1
```

### Partial Fill Path

User2 has 3 SOL at $100. User1 buys 5 SOL at $100. Only 3 match.

```
match_bid() fills 3 SOL (all of User2's ask) → executed_qty = 3.0
executed_qty (3.0) < order.quantity (5.0)
→ order IS added to bids at $100 for remaining 2 SOL
→ update_balance() processes only the 3 SOL fill
→ depth broadcast: $100 ask now empty, $100 bid now has 2 SOL
→ User1's order sits in book waiting for more sellers
```

### No Match Path

No asks exist at or below $100. `match_bid()` returns `([], 0.0)`. The order is added to bids. No fills, no trades, no balance changes. Only a depth broadcast showing the new $100 bid level.

### Cancel Path

```
CancelOrder { order_id: "xyz", market: "SOL_USDC" }
→ find SOL_USDC orderbook
→ find the order in get_open_orders()
→ order.side == Buy → cancel_bid("xyz") → returns price
→ left_quantity = (order.quantity - order.filled) * order.price
→ locked -= left_quantity
→ available += left_quantity   (funds returned to user)
→ send_updated_depth_at(price) → broadcast updated book
```

---

## 5. Every Function Explained

### `Engine::new()`

```rust
pub fn new() -> Self {
    let mut engine = Self {
        orderbooks: vec![Orderbook::new("SOL_USDC".to_string())],
        balances: HashMap::new(),
    };
    engine.set_base_balances();
    engine
}
```

Constructor. Creates the engine with one hardcoded orderbook. `set_base_balances()` is an empty stub — balances are created lazily when users first interact via `ensure_user_balance()`.

`vec![x]` is shorthand for `Vec::new()` + `.push(x)`. Creates a Vec with initial elements.

---

### `ensure_user_balance()`

```rust
fn ensure_user_balance(&mut self, user_id: &str) {
    if !self.balances.contains_key(user_id) {
        let mut user_balance = HashMap::new();
        for currency in ["SOL", "USDC", "INR"].iter() {
            user_balance.insert(currency.to_string(), UserBalance {
                available: 10_000_000.0,
                locked: 0.0,
            });
        }
        self.balances.insert(user_id.to_string(), user_balance);
    }
}
```

**Lazy initialization pattern.** You can't pre-create balances for all users because you don't know who will show up. So you create a user's balance record the first time they place an order.

`10_000_000.0` — the underscore is Rust's numeric separator for readability. Same as `10000000.0`. This is testnet faucet money; production would start at `0.0`.

---

### `process()` — The Main Dispatcher

```rust
pub fn process(&mut self, message: MessageFromApi, client_id: String, user_id: String) {
    match message {
        MessageFromApi::CreateOrder { data }          => { ... }
        MessageFromApi::CancelOrder { order_id, .. }  => { ... }
        MessageFromApi::GetOpenOrders { data }        => { ... }
        MessageFromApi::OnRamp { amount, user_id, .. }=> { ... }
        MessageFromApi::GetDepth { data }             => { ... }
    }
}
```

This is the router. Every message that comes off the Redis queue lands here first. `MessageFromApi` is a Rust enum where each variant carries different data. The `..` in pattern matching means "ignore the remaining fields".

Think of `process()` as the telephone operator at an old-school switchboard — she doesn't do the work, she routes the call to whoever does.

---

### `create_order()` — The Core Function

This is the most important function. Everything leads here.

```rust
pub fn create_order(
    &mut self,
    market: &str,
    price: &str,       // comes in as string from JSON
    quantity: &str,    // comes in as string from JSON
    side: OrderSide,
    user_id: &str,
) -> Result<(f64, Vec<Fill>, String), String>
//          ^executed_qty  ^fills  ^order_id   ^error
```

**Step by step:**

| Step | Code | What it does |
|------|------|--------------|
| 1 | `ensure_user_balance(user_id)` | Lazy init user's balance |
| 2 | `orderbooks.iter_mut().find(...)` | Find the mutable orderbook |
| 3 | `thread_rng().sample_iter(...)` | Generate 26-char random order ID |
| 4 | `price.parse::<f64>()` | Parse string → number |
| 5 | Build `Order` struct | Assemble the order |
| 6 | `orderbook.add_order(&mut order)` | Attempt matching |
| 7 | `update_balance()` | Adjust ledger for fills |
| 8 | `create_db_trades()` | Queue DB write |
| 9 | `update_db_orders()` | Queue order status update |
| 10 | `publish_ws_depth_updates()` | Broadcast book change |
| 11 | `publish_ws_trades()` | Broadcast trade event |
| 12 | `Ok((executed_qty, fills, order_id))` | Return result |

**Key insight:** The function does two separate things. (1) Pure in-memory matching via `orderbook`. (2) Side effects — persistence and notifications. The orderbook is pure state. Everything else is async fire-and-forget.

---

### `update_balance()` — The Accounting Logic

This is the trickiest function. When orders match, money moves between TWO users.

**Concrete example:**
- User1 (buyer) placed a resting bid at $100 for 5 SOL — their USDC is already locked
- User2 (seller) comes in with a market sell that matches
- One `Fill` is generated: `{ qty: 5, price: 100, other_user_id: "user1" }`

```
For OrderSide::Buy (User2 is the incoming seller, fill.other_user_id = User1 the waiting buyer):

other_user (User1, the buyer):
  quote_balance.locked -= fill.qty * fill.price   ← unlock their reserved USDC
  base_balance.locked  -= fill.qty                ← their SOL was "locked" on sell side

current_user (User2, the seller):
  quote_balance.available += fill.qty * fill.price ← receive USDC payment
  base_balance.locked     -= fill.qty              ← SOL they had locked for the sell
```

> ⚠️ **Note:** The current code passes `market` (e.g. `"SOL_USDC"`) as both `base_asset` and `quote_asset`. This means balance lookups silently fail via the `if let Some(...)` guards since no user has a `"SOL_USDC"` currency key. The matching engine is the showcase — the accounting layer is a known simplification in this portfolio project.

---

### `create_db_trades()`

```rust
fn create_db_trades(&mut self, fills: &Vec<Fill>, market: &str, user_id: &str) {
    for fill in fills {
        let conn = RedisManager::get_instance().lock().unwrap();
        let message = DbMessage::TradeAdded(TradeMessage {
            id: fill.trade_id.to_string(),
            is_buyer_maker: fill.other_user_id == user_id,
            price: fill.price.to_string(),
            quantity: fill.qty.to_string(),
            quote_quantity: (fill.qty * fill.price).to_string(),
            timestamp: SystemTime::now()...as_millis() as i64,
            market: market.to_string(),
        });
        conn.push_message_to_db_processor(message);
    }
}
```

For every fill, push a trade record to a separate Redis queue consumed by the DB processor service. The engine doesn't write to Postgres directly — it delegates to avoid blocking on slow DB writes.

`is_buyer_maker` — true when the resting order (already in the book) was the buy side. Used for exchange analytics and maker/taker fee models.

---

### `publish_ws_depth_updates()`

After a fill, the visible order book changes. This broadcasts only the price levels that were touched — a delta update, not a full snapshot. The frontend merges deltas into its local book state.

```rust
// For a Buy order: the ask levels that were consumed + the current bid level
let updated_asks: Vec<[String; 2]> = depth.asks
    .into_iter()
    .filter(|(p, _)| fill_prices.contains(p))  // only changed levels
    .map(|(p, q)| [p, q])
    .collect();
```

---

### `publish_ws_trades()`

Broadcasts every fill as a trade event to `trade@SOL_USDC` channel.

```rust
WsMessage {
    stream: format!("trade@{}", market),
    data: WsMessageData::Trade(TradeData {
        e: "trade".to_string(),     // event type
        t: fill.trade_id,           // monotonically incrementing trade ID
        m: fill.other_user_id == user_id,  // is the resting order's user the buyer?
        p: fill.price.to_string(),
        q: fill.qty.to_string(),
        s: market.to_string(),
    }),
}
```

---

### `on_ramp()`

```rust
fn on_ramp(&mut self, user_id: &str, amount: f64) {
    if let Some(user_balance) = self.balances.get_mut(user_id) {
        if let Some(base_balance) = user_balance.get_mut(BASE_CURRENCY) {
            base_balance.available += amount;
        } else {
            user_balance.insert(BASE_CURRENCY.to_string(), UserBalance {
                available: amount, locked: 0.0
            });
        }
    } else {
        // user doesn't exist at all — create full record
        let mut new_balance = HashMap::new();
        new_balance.insert(BASE_CURRENCY.to_string(), UserBalance {
            available: amount, locked: 0.0
        });
        self.balances.insert(user_id.to_string(), new_balance);
    }
}
```

OnRamp = depositing money. `BASE_CURRENCY` is `"INR"`. Handles three cases: user exists with INR key, user exists without INR key, user doesn't exist at all.

---

### `check_and_lock_funds()`

Currently called nowhere but the logic is correct. In a production exchange this would run BEFORE `add_order()` to reject orders when a user has insufficient funds, preventing phantom orders from entering the book.

```rust
OrderSide::Buy => {
    let required = price * quantity;
    if asset_balance.available < required {
        return Err("Insufficient funds".to_string());
    }
    asset_balance.available -= required;
    asset_balance.locked    += required;
}
```

---

### `save_snapshot()`

```rust
pub fn save_snapshot(&self) {
    let snapshot = serde_json::json!({
        "orderbooks": self.orderbooks.iter().map(|o| o.get_snapshot()).collect::<Vec<_>>(),
        "balances": self.balances.clone(),
    });
    fs::write("./snapshot.json", serde_json::to_string_pretty(&snapshot).unwrap()).unwrap();
}
```

Serializes the entire engine state to disk. In production, you'd load this on startup to restore state after a restart. The `serde_json::json!()` macro builds a JSON value from Rust expressions inline.

---

## 6. Rust Concepts Used

### `Arc<Mutex<T>>` — The Thread-Safety Wrapper

This is the most confusing pattern for beginners. Let's kill the mystery.

```
Arc<Mutex<RedisManager>>
 │    │
 │    └── Mutual Exclusion: only ONE thread can access the data at a time
 └─────── Atomically Reference Counted: multiple owners, freed when count hits zero
```

**Real-world analogy:** A shared office printer.
- `Arc` = everyone in the office has the printer's IP address (multiple owners of the reference)
- `Mutex` = only one person can print at a time (exclusive access)
- `.lock()` = you click Print and wait for the printer to be free
- `.unwrap()` = assume it worked (panics if the mutex is "poisoned" — another thread panicked while holding the lock)
- The guard drops automatically when it goes out of scope — you never manually unlock

```rust
// Pattern used throughout engine.rs:
let conn = RedisManager::get_instance()  // Arc<Mutex<RedisManager>>
    .lock()                              // blocks until lock is free, returns MutexGuard
    .unwrap();                           // unwrap the Result (panics if poisoned)
conn.send_to_api(...);                   // exclusive access to RedisManager
// conn drops here → lock released automatically
```

> ⚠️ **Never hold a Mutex lock while doing slow I/O.** Lock, use, drop. Keep it tight.

---

### `Option<T>` and `Result<T, E>`

```rust
// Option: something that might not exist
let orderbook: Option<&Orderbook> = self.orderbooks.iter().find(|o| o.ticker() == market);
match orderbook {
    Some(ob) => { /* use it */ }
    None     => { /* no such market */ }
}

// Shorthand with if let (when you only care about Some):
if let Some(ob) = self.orderbooks.iter().find(|o| o.ticker() == market) {
    // ob is available here
}

// Result: something that might fail
match self.create_order(...) {
    Ok((qty, fills, id)) => { /* success */ }
    Err(e)               => { /* send error to API */ }
}

// The ? operator: return early on Err, unwrap on Ok
let price = price.parse::<f64>().map_err(|_| "Invalid price")?;
//                                                             ^ if Err, return from function immediately
```

---

### `iter()` vs `iter_mut()` vs `into_iter()`

| Method | Gives you | Collection after? |
|--------|-----------|-------------------|
| `.iter()` | `&T` (read-only borrow) | Still exists |
| `.iter_mut()` | `&mut T` (mutable borrow) | Still exists |
| `.into_iter()` | `T` (ownership) | Consumed, gone |

```rust
// iter() — just reading, collection unchanged
self.orderbooks.iter().find(|o| o.ticker() == market)

// iter_mut() — need to modify the orderbook
self.orderbooks.iter_mut().find(|o| o.ticker() == market)

// into_iter() — transform and consume
depth.bids.into_iter().filter(...).map(...).collect()
//         ^ depth.bids is GONE after this line
```

---

### Closures: `|x| x.something()`

Anonymous functions defined inline. The `||` is Rust's lambda syntax.

```rust
.find(|o| o.ticker() == market)          // single param
.filter(|(p, _)| fill_prices.contains(p)) // destructuring a tuple
.map(|(p, q)| [p, q])                    // transform tuple to array
.map_err(|_| "Invalid price")            // ignore the error value, return string
```

Python equivalent: `lambda o: o.ticker() == market`

---

### `String` vs `&str`

```rust
String        // owned, heap-allocated, you can mutate it
&str          // borrowed view into string data, cannot outlive what it borrows from
&String       // reference to an owned String (coerces to &str automatically)

// In function params: prefer &str (more flexible, accepts both String and &str)
fn find_market(market: &str) -> Option<&Orderbook>

// In struct fields: use String (needs to own the data)
struct Engine {
    market: String,
}

// Conversion:
"SOL_USDC".to_string()     // &str → String
my_string.as_str()         // String → &str
&my_string                 // String → &String (auto-deref to &str in most contexts)
```

---

### The `format!()` macro

```rust
format!("trade@{}", market)    // → "trade@SOL_USDC"
format!("depth@{}", market)    // → "depth@SOL_USDC"
```

Same as Python's f-strings: `f"trade@{market}"`. Returns an owned `String`.

---

## 7. Where You Will Get Stuck

### Stuck Point 1: Borrow Checker — Cannot borrow `self` as mutable and immutable

**The error:**
```
error[E0502]: cannot borrow `self` as mutable because it is also borrowed as immutable
```

**When it happens:** You hold a reference to something inside `self` (like `&Orderbook`) and then try to call a `&mut self` method.

**Broken pattern:**
```rust
let orderbook = self.orderbooks.iter_mut().find(|o| o.ticker() == market);
let (fills, qty) = orderbook.unwrap().add_order(&mut order)?;
self.update_balance(...);  // ERROR: self still borrowed via `orderbook`
```

**Fix — wrap the mutable borrow in a block so it drops before the next call:**
```rust
let (fills, executed_qty) = {
    let orderbook = self.orderbooks
        .iter_mut()
        .find(|o| o.ticker() == market)
        .ok_or_else(|| format!("No orderbook for {}", market))?;
    orderbook.add_order(&mut order)?
};  // <-- orderbook borrow DROPS here, fills/executed_qty are owned values

self.update_balance(...);  // now self is free
```

This is exactly why `create_order()` works — the block scope is doing the heavy lifting.

---

### Stuck Point 2: Mutex Deadlock

**The problem:** You lock `RedisManager`, then try to lock it again in the same scope. Thread hangs forever.

```rust
// WRONG — deadlock:
let conn = RedisManager::get_instance().lock().unwrap();
conn.push_message_to_db_processor(msg1);
let conn2 = RedisManager::get_instance().lock().unwrap();  // HANGS FOREVER
```

**Fix:** Either use one connection for multiple calls, or explicitly drop between:
```rust
// OK — one lock, multiple uses:
let conn = RedisManager::get_instance().lock().unwrap();
conn.push_message_to_db_processor(msg1);
conn.push_message_to_db_processor(msg2);
// conn drops at end of scope

// OK — explicit drop:
{
    let conn = RedisManager::get_instance().lock().unwrap();
    conn.push_message_to_db_processor(msg1);
} // conn drops here, lock released
{
    let conn = RedisManager::get_instance().lock().unwrap();
    conn.send_to_api(...);
}
```

---

### Stuck Point 3: `into_iter()` consuming your data

```rust
let depth = orderbook.get_depth();

// This CONSUMES depth.bids:
let updated_asks = depth.asks.into_iter().filter(...).collect();

// Now this FAILS — depth.bids was moved:
let updated_bids = depth.bids.into_iter()...  // ERROR: value used after move
```

**Fix:** Use `.iter()` to borrow instead, or call `get_depth()` again, or clone the field you need before consuming:
```rust
let bids_clone = depth.bids.clone();
let updated_asks = depth.asks.into_iter()...;
let updated_bids = bids_clone.into_iter()...;
```

---

### Stuck Point 4: `parse::<f64>()` Silently Failing

Price and quantity arrive as `&str` from JSON. Common parse failures:

```rust
"100.5 ".parse::<f64>()    // Err! trailing space
"100,5".parse::<f64>()     // Err! comma decimal separator
"".parse::<f64>()           // Err! empty string
"100.5".parse::<f64>()     // Ok(100.5)
```

**Always trim:**
```rust
let price = price.trim().parse::<f64>().map_err(|_| "Invalid price")?;
```

---

### Stuck Point 5: Nested HashMap access chain

```rust
// If ANY key doesn't exist, the whole chain returns None — silently
if let Some(balance) = self.balances.get_mut(&fill.other_user_id) {
    if let Some(asset_balance) = balance.get_mut(quote_asset) {
        asset_balance.available += fill.qty * fill.price;
    }
    // If quote_asset key doesn't exist → update silently skipped, no error
}
```

In the portfolio code this is intentional (avoid panics). In production, you'd propagate an error:
```rust
let balance = self.balances.get_mut(&fill.other_user_id)
    .ok_or("User balance not found")?;
let asset = balance.get_mut(quote_asset)
    .ok_or("Asset balance not found")?;
asset.available += fill.qty * fill.price;
```

---

### Stuck Point 6: The `..` in enum pattern matching

```rust
MessageFromApi::CancelOrder { order_id, market, .. } => { ... }
//                                                 ^^ ignores all other fields
```

Without `..`, you must list every field the enum variant contains. With `..`, you only destructure what you need.

---

### Stuck Point 7: `&Vec<Fill>` vs `&[Fill]`

```rust
fn create_db_trades(&mut self, fills: &Vec<Fill>, ...)  // works but Clippy warns
fn create_db_trades(&mut self, fills: &[Fill], ...)     // idiomatic — prefer this
```

`&Vec<T>` coerces to `&[T]` anyway. Using `&[Fill]` is more flexible (accepts Vec, array, slice).

---

## 8. Build Plan — 100 Steps

Follow this order. Have a compiling project at every step. Never go 20+ lines without `cargo check`.

### Phase 1: Scaffolding (Steps 1–15)

1. `cargo new exchange-engine --lib`
2. Add to `Cargo.toml`: `serde = { version = "1", features = ["derive"] }`
3. Add: `serde_json = "1"`, `rand = "0.8"`, `log = "0.4"`, `env_logger = "0.10"`
4. Add: `redis = "0.23"` (or whatever version your project uses)
5. Create `src/trade/mod.rs` and `src/trade/orderbook.rs` (your existing orderbook)
6. Create `src/redis/mod.rs` and `src/redis/redis_manager.rs` (stub file only)
7. Create `src/types/mod.rs`, `src/types/api.rs`, `src/types/ws.rs`
8. Define `OrderSide` enum in `redis_manager.rs`: `Buy`, `Sell` — derive `Debug, Clone, PartialEq, Serialize, Deserialize`
9. Define `MessageFromApi` enum in `types/api.rs` — 5 variants with their data structs
10. Define `MessageToApi` enum: `OrderPlaced`, `Error`, `OpenOrders`, `Depth`
11. Define `DepthPayload` struct: `{ market: String, bids: Vec<(String,String)>, asks: Vec<(String,String)> }`
12. Define `WsMessage`, `WsMessageData`, `TradeData`, `DepthData` in `types/ws.rs`
13. Define `DbMessage`, `OrderMessage`, `TradeMessage` in `redis_manager.rs`
14. Create `src/trade/engine.rs` — just the `use` statements and module declaration
15. Add `pub const BASE_CURRENCY: &str = "INR";` at the top of `engine.rs`

### Phase 2: Core Structs (Steps 16–25)

16. Define `UserBalance` struct — `available: f64`, `locked: f64` — derive `Debug, Clone, Serialize, Deserialize`
17. Define `Engine` struct with `orderbooks: Vec<Orderbook>` and `balances: HashMap<String, HashMap<String, UserBalance>>`
18. Add `impl Engine { }` block
19. Implement `Engine::new()` — create engine with `SOL_USDC` orderbook, empty balances HashMap
20. Call `self.set_base_balances()` in `new()` — implement as empty function with a comment
21. Implement `ensure_user_balance(&mut self, user_id: &str)`
22. Inside: `if !self.balances.contains_key(user_id) { }`
23. Inside the if: create `user_balance: HashMap<String, UserBalance>`
24. Loop `["SOL", "USDC", "INR"]` — insert each with `available: 10_000_000.0, locked: 0.0`
25. `self.balances.insert(user_id.to_string(), user_balance)` — run `cargo check`

### Phase 3: process() Dispatcher (Steps 26–35)

26. Write `process()` signature: `pub fn process(&mut self, message: MessageFromApi, client_id: String, user_id: String)`
27. Add `match message { }` block
28. Add `MessageFromApi::CreateOrder { data } => { }` arm — call `self.create_order(...)` (return stub `Ok(...)` for now)
29. Handle `Ok` arm: lock Redis, call `send_to_api` with `MessageToApi::OrderPlaced { order_id, executed_qty, fills }`
30. Handle `Err` arm: lock Redis, call `send_to_api` with `MessageToApi::Error { message: e }`
31. Add `MessageFromApi::CancelOrder { order_id, market, .. } => { }` arm
32. Add `MessageFromApi::GetOpenOrders { data } => { }` arm — find orderbook, get orders, send via Redis
33. Add `MessageFromApi::OnRamp { amount, user_id, .. } => { }` arm — parse amount, call `self.on_ramp()`
34. Add `MessageFromApi::GetDepth { data } => { }` arm — find orderbook, call `get_depth()`, send via Redis
35. Add `None` fallback arms for all orderbook lookups — run `cargo check`

### Phase 4: create_order() (Steps 36–50)

36. Write `create_order()` signature returning `Result<(f64, Vec<Fill>, String), String>`
37. First line: `self.ensure_user_balance(user_id);`
38. Find orderbook: `self.orderbooks.iter_mut().find(|o| o.ticker() == market).ok_or_else(|| format!(...))?`
39. Generate `order_id`: `thread_rng().sample_iter(&Alphanumeric).take(26).map(char::from).collect()`
40. Parse price: `price.parse::<f64>().map_err(|_| "Invalid price")?`
41. Parse quantity: `quantity.parse::<f64>().map_err(|_| "Invalid quantity")?`
42. Build `Order { price, quantity, order_id: order_id.clone(), filled: 0.0, side: side.clone(), user_id: user_id.to_string() }`
43. **Key:** wrap the orderbook borrow in a block: `let (fills, executed_qty) = { let ob = ...; ob.add_order(&mut order)? };`
44. Call `self.update_balance(user_id, market, market, &side, &fills, executed_qty)?`
45. Call `self.create_db_trades(&fills, market, user_id)`
46. Call `self.update_db_orders(&order, executed_qty, &fills, market)`
47. Call `self.publish_ws_depth_updates(&fills, price.to_string(), &side, market)`
48. Call `self.publish_ws_trades(&fills, user_id, market)`
49. Return `Ok((executed_qty, fills, order_id))`
50. Write unit test: create engine, call `create_order` with no matching orders — verify it returns `Ok((0.0, [], _))`

### Phase 5: update_balance() (Steps 51–60)

51. Write signature: `fn update_balance(&mut self, user_id: &str, base_asset: &str, quote_asset: &str, side: &OrderSide, fills: &[Fill], _executed_qty: f64) -> Result<(), String>`
52. `match side { OrderSide::Buy => { } OrderSide::Sell => { } }`
53. In Buy arm: `for fill in fills { }`
54. Get other user's balance: `if let Some(other_balance) = self.balances.get_mut(&fill.other_user_id) { }`
55. Update other user's quote balance: `quote_balance.available += fill.qty * fill.price`
56. Update current user's quote locked: `quote_balance.locked -= fill.qty * fill.price`
57. Update other user's base locked: `base_balance.locked -= fill.qty`
58. Update current user's base available: `base_balance.available += fill.qty`
59. Mirror the logic for `OrderSide::Sell` (seller gives base, receives quote)
60. Return `Ok(())` — write test: two matching orders, verify balances change correctly

### Phase 6: DB and WebSocket Publishing (Steps 61–75)

61. Implement `update_db_orders()`: lock Redis, build `DbMessage::OrderUpdate` for the incoming order
62. In `update_db_orders()`: loop `fills`, push `DbMessage::OrderUpdate` for each `fill.marker_order_id`
63. Implement `create_db_trades()`: loop `fills`, compute `quote_qty = fill.qty * fill.price`
64. Build `TradeMessage` with `SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64` timestamp
65. Push `DbMessage::TradeAdded` to Redis DB processor queue
66. Implement `publish_ws_trades()`: loop `fills`
67. Build `WsMessage { stream: format!("trade@{}", market), data: WsMessageData::Trade(TradeData{...}) }`
68. Set `m: fill.other_user_id == user_id` in TradeData
69. Call `conn.publish_message_to_ws(&format!("trade@{}", market), message)`
70. Implement `publish_ws_depth_updates()`: get mutable orderbook reference, call `get_depth()`
71. Collect `fill_prices: Vec<String>` from fills
72. In `OrderSide::Buy` arm: filter asks to fill_prices, filter bids to current price, build WsMessage
73. In `OrderSide::Sell` arm: filter bids to fill_prices, filter asks to current price, build WsMessage
74. Call `conn.publish_message_to_ws(&format!("depth@{}", market), message)`
75. Implement `send_updated_depth_at()` for cancel order depth broadcasts

### Phase 7: on_ramp and Cancel (Steps 76–85)

76. Implement `on_ramp()`: `if let Some(user_balance) = self.balances.get_mut(user_id)`
77. Inside: `if let Some(base_balance) = user_balance.get_mut(BASE_CURRENCY)` → `base_balance.available += amount`
78. Else arm: `user_balance.insert(BASE_CURRENCY.to_string(), UserBalance { available: amount, locked: 0.0 })`
79. Outer else: create full user entry with just `BASE_CURRENCY` key and amount
80. Implement `check_and_lock_funds()` — even though it's unused, it documents the intended pre-order flow
81. In `CancelOrder` handler: `if let Some(orderbook) = self.orderbooks.iter_mut().find(...)`
82. Call `orderbook.get_open_orders(&order_id)` — note: this searches by user_id field, so this is technically passing order_id where user_id is expected
83. For Buy cancel: `orderbook.cancel_bid(&order_id)` → compute `left_quantity = (order.quantity - order.filled) * order.price`
84. Refund: `asset_balance.available += left_quantity; asset_balance.locked -= left_quantity`
85. Call `self.send_updated_depth_at(price, &market)` after successful cancel

### Phase 8: Polish and Tests (Steps 86–100)

86. Implement `save_snapshot()` using `serde_json::json!()` macro and `fs::write()`
87. Implement `add_orderbook()` to allow adding markets dynamically
88. Replace remaining `unwrap()` calls with `?` or proper error handling where possible
89. Add `log::info!` calls at function entry/exit matching the existing pattern
90. Test: `test_create_order_no_match` — buy with no matching asks, verify order in bids
91. Test: `test_create_order_full_match` — buy then matching sell, verify both cleared from book
92. Test: `test_create_order_partial_match` — buy 10, sell 5, verify 5 remains in bids
93. Test: `test_on_ramp_new_user` — call `on_ramp` for unknown user, verify balance created
94. Test: `test_on_ramp_existing_user` — call `on_ramp` twice, verify amounts accumulate
95. Test: `test_ensure_user_balance` — new user gets 10M of each currency
96. Test: `test_cancel_bid` — place order, cancel it, verify it's removed from book
97. Integration test: `test_full_order_lifecycle` — onramp → buy → partial fill → cancel remainder
98. Run `cargo clippy` and fix all warnings
99. Run `cargo fmt` to format
100. Run `cargo test -- --nocapture` and verify all tests pass with clean log output

---

## 9. Mental Model Cheatsheet

Burn this into memory. Quiz yourself on it before writing a line.

### The Three Layers

```
Layer 1 — Transport    Redis queues          Messages in, responses out. Engine never does HTTP.
Layer 2 — Engine       engine.rs             Single-threaded brain. Owns ALL state. Routes messages.
Layer 3 — Side Effects DB queue + WS pub/sub Fire-and-forget. Non-blocking from engine's perspective.
```

### State Ownership Rules

```
self.orderbooks  ← ONLY source of truth for open orders. Nothing else.
self.balances    ← ONLY source of truth for user funds. Nothing else.
Redis            ← message transport only. NOT state storage.
Postgres         ← eventual persistence via DB processor. NOT authoritative.
```

### The Borrow Checker Rule for This File

> If you hold a reference into `self.orderbooks`, you cannot call any `&mut self` method.
> Wrap the borrow in a block `{ }` to drop it before the next call.

### Order Lifecycle

```
New Order
    │
    ▼
add_order()
    │
    ├── match found? ──YES──► generate Fill(s)
    │                              │
    │                              ▼
    │                        update_balance()   ← ledger
    │                        create_db_trades() ← persistence queue
    │                        update_db_orders() ← persistence queue
    │                        publish_ws_trades() ← live feed
    │                        publish_ws_depth()  ← live feed
    │
    └── no match / partial ──► add remaining to orderbook (bids or asks BTreeMap)
```

### Key Data Types Quick Reference

| Type | What it is | Example |
|------|-----------|---------|
| `String` | Owned heap string | `"SOL_USDC".to_string()` |
| `&str` | Borrowed string slice | `fn f(market: &str)` |
| `Vec<T>` | Growable array | `Vec<Orderbook>` |
| `HashMap<K,V>` | Hash table | `HashMap<String, UserBalance>` |
| `BTreeMap<K,V>` | Sorted tree map | `BTreeMap<Price, Vec<Order>>` (in orderbook) |
| `Option<T>` | Maybe has a value | `Some(x)` or `None` |
| `Result<T,E>` | Success or failure | `Ok(x)` or `Err(e)` |
| `Arc<Mutex<T>>` | Thread-safe shared ownership | `RedisManager` singleton |

### Closure Cheatsheet

```rust
.find(|x| x.ticker() == market)          // find first match
.filter(|(p, _)| prices.contains(p))     // keep matching elements (destructure tuple)
.map(|(p, q)| [p, q])                    // transform each element
.map_err(|_| "error string")             // transform the error (ignore original)
.collect::<Vec<_>>()                     // collect iterator into Vec
.ok_or_else(|| format!("msg {}", x))     // Option → Result
```

### The ? Operator

```rust
// Without ?:
let price = match price.parse::<f64>() {
    Ok(p) => p,
    Err(_) => return Err("Invalid price".to_string()),
};

// With ?:
let price = price.parse::<f64>().map_err(|_| "Invalid price")?;
// If Err: immediately returns Err from the function
// If Ok: unwraps and binds the value
```

---

*Built as part of a Rust crypto exchange portfolio project demonstrating production-grade matching engine design.*