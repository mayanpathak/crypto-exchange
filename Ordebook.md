# Orderbook Engine — Complete Reference & Build Guide

> A complete deep-dive into a Rust limit-order-book (LOB) matching engine.
> This document covers architecture, every line of logic, Rust concepts, mental models, and a 100-step build plan to reconstruct the file from scratch.

---

## Table of Contents

1. [What Is a Limit Order Book?](#1-what-is-a-limit-order-book)
2. [High-Level Architecture](#2-high-level-architecture)
3. [Rust Concepts You Must Understand First](#3-rust-concepts-you-must-understand-first)
4. [Dependency & Import Block](#4-dependency--import-block)
5. [The Price Newtype](#5-the-price-newtype)
6. [The Fill Struct](#6-the-fill-struct)
7. [The Order Struct](#7-the-order-struct)
8. [The Orderbook Struct](#8-the-orderbook-struct)
9. [The OrderbookSnapshot Struct](#9-the-orderbooksnapshot-struct)
10. [Function: new()](#10-function-new)
11. [Function: ticker()](#11-function-ticker)
12. [Function: get_snapshot()](#12-function-get_snapshot)
13. [Function: add_order()](#13-function-add_order)
14. [Function: match_bid()](#14-function-match_bid)
15. [Function: match_ask()](#15-function-match_ask)
16. [Function: get_depth()](#16-function-get_depth)
17. [Function: get_open_orders()](#17-function-get_open_orders)
18. [Function: cancel_bid()](#18-function-cancel_bid)
19. [Function: cancel_ask()](#19-function-cancel_ask)
20. [The Test Suite — Every Test Explained](#20-the-test-suite--every-test-explained)
21. [Data Flow — End to End](#21-data-flow--end-to-end)
22. [Common Patterns Used in This File](#22-common-patterns-used-in-this-file)
23. [Edge Cases & Gotchas](#23-edge-cases--gotchas)
24. [What This File Is Missing (Production Gaps)](#24-what-this-file-is-missing-production-gaps)
25. [The 100-Step Build Plan](#25-the-100-step-build-plan)

---

## 1. What Is a Limit Order Book?

A **limit order book (LOB)** is the central mechanism of every financial exchange in the world — from the New York Stock Exchange to Binance to a local fish auction. It records every open buy and sell order and automatically executes trades when prices agree.

### The Hotel Noticeboard Analogy

Imagine a ski resort hotel with a noticeboard split into two halves:

- **Left side (Bids):** Guests post "I'll pay $100 for a lift ticket." Everyone can see all offers. The person offering the most is on top.
- **Right side (Asks):** The resort posts "Lift tickets available for $103." The cheapest ticket is on top.
- **The Concierge (Matching Engine):** Watches both sides continuously. The moment a buyer's price meets or beats a seller's price, the concierge pulls both notes, shakes hands between buyer and seller, stamps a receipt ("Fill"), and the trade is done.
- **Partial fills:** If a buyer wants 10 tickets but only 6 are available at the right price, they get 6 immediately and a new note goes on the board for the remaining 4.

This is *exactly* what this Rust file implements.

### Key Terms

| Term | Definition | Example |
|---|---|---|
| **Bid** | An open buy order | "Buy 5 BTC at $100 max" |
| **Ask** | An open sell order | "Sell 5 BTC at $103 min" |
| **Spread** | Gap between best bid and best ask | $103 - $100 = $3 |
| **Fill** | A completed trade | "5 BTC traded at $103" |
| **Maker** | Order resting on the book | The old buy order waiting |
| **Taker** | Incoming order that hits the book | The new sell order that matched |
| **Partial fill** | Order only partly matched | Wanted 10, got 6 |
| **Market depth** | Aggregated volume at each price | "500 BTC available at $100" |

---

## 2. High-Level Architecture

```
                         ┌─────────────────────────────┐
                         │         Orderbook           │
                         │  market: "BTC_USDT"         │
                         │  last_trade_id: 42          │
                         │  current_price: 103.0       │
                         │                             │
                         │  ┌──────────┐ ┌──────────┐  │
                         │  │  bids    │ │  asks    │  │
                         │  │ BTreeMap │ │ BTreeMap │  │
                         │  │          │ │          │  │
                         │  │ $102 ──► │ │ $103 ──► │  │
                         │  │ [O, O]   │ │ [O]      │  │
                         │  │ $101 ──► │ │ $104 ──► │  │
                         │  │ [O]      │ │ [O, O]   │  │
                         │  │ $100 ──► │ │ $105 ──► │  │
                         │  │ [O, O, O]│ │ [O]      │  │
                         │  └──────────┘ └──────────┘  │
                         │                             │
                         │  orders: HashMap<id, Order> │
                         └─────────────────────────────┘
                                    │
                     ┌──────────────┴──────────────┐
                     │                             │
                add_order(Buy)             add_order(Sell)
                     │                             │
               match_bid()                   match_ask()
               (scan asks,                 (scan bids,
                low→high)                   high→low)
                     │                             │
                     └──────────────┬──────────────┘
                                    │
                              Vec<Fill>
                         (trade receipts returned
                          to caller for settlement)
```

### Data Structure Choice: Why BTreeMap?

A `BTreeMap` keeps its keys in **sorted order at all times** with O(log n) insert and lookup. This is critical for a matching engine:

- We always need the **highest bid** (best buyer price) — it lives at the maximum key.
- We always need the **lowest ask** (best seller price) — it lives at the minimum key.
- With `BTreeMap`, both are O(log n) operations.
- With `HashMap`, we'd need O(n) scans to find the best price every time.

The tradeoff: `BTreeMap` has slower constant factors than `HashMap`. For a matching engine where price priority is the core invariant, this is the correct tradeoff.

---

## 3. Rust Concepts You Must Understand First

Before reading a single function, internalize these Rust ideas. Each one appears repeatedly in this file.

---

### 3.1 Ownership and `&mut self`

Rust has a rule: **exactly one owner per value at a time**. When a function takes `&mut self`, it borrows the whole struct mutably — meaning it can modify fields, but no one else can touch the struct at the same time.

```rust
pub fn add_order(&mut self, order: &mut Order) -> ...
//               ^^^^ mutable borrow of Orderbook
//                                  ^^^^ mutable borrow of the incoming Order
```

**Layman version:** `&mut self` is like checking out the master key to a hotel. You have full write access, but reception can't give it to anyone else until you return it.

---

### 3.2 The Newtype Pattern — `Price(f64)`

```rust
pub struct Price(f64);
```

This wraps `f64` (a floating-point number) in a struct. Reason: Rust's standard `f64` does not implement `Ord` (total ordering) because floats have `NaN` (Not a Number), which breaks any comparison. `BTreeMap` requires keys to implement `Ord`. Solution: wrap in a struct and implement `Ord` manually.

**Layman version:** You can't alphabetically sort a pile of fish (slippery, shapeless). Put each fish in a labeled box. Now you can sort boxes alphabetically. `Price(f64)` is the box.

---

### 3.3 Traits — `Eq`, `PartialEq`, `PartialOrd`, `Ord`

Traits are Rust's version of interfaces. To use `Price` as a `BTreeMap` key, you must implement all four:

| Trait | Meaning | Required because |
|---|---|---|
| `PartialEq` | `a == b` might not always be defined (NaN) | Basic equality |
| `Eq` | `a == a` always (no NaN nonsense) | BTreeMap needs this guarantee |
| `PartialOrd` | `a < b` comparison that might fail | Basis for Ord |
| `Ord` | Total ordering — always returns Less/Equal/Greater | BTreeMap sorting |

```rust
impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}
```

The `unwrap_or(Ordering::Equal)` handles the NaN case gracefully — if comparison fails, call them equal.

---

### 3.4 `Result<T, E>` — Error Handling

```rust
pub fn add_order(&mut self, order: &mut Order) -> Result<(Vec<Fill>, f64), String>
```

`Result` is Rust's way of saying "this might fail." It's either:
- `Ok((fills, executed_qty))` — success with values
- `Err("some error message")` — failure with description

The `?` operator is shorthand for "if this returns Err, immediately return that Err from the current function." Example:

```rust
let (fills, executed_qty) = self.match_bid(order)?;
//                                               ^ if match_bid returns Err, bubble it up
```

**Layman version:** `Result` is a package that either contains a gift (`Ok`) or a note saying why there's no gift (`Err`). The `?` is like saying "if the package has a sorry note, forward it immediately to the person who asked."

---

### 3.5 `Vec<T>` — A Dynamic List

`Vec<Order>` is a growable array of `Order` values. In this file, each price level in the BTreeMap holds a `Vec<Order>` — multiple orders can sit at the same price (they're time-prioritized, first-in first-served).

Key operations used:
- `vec.push(order)` — add to end
- `vec.iter()` — read each item
- `vec.iter_mut()` — read and modify each item
- `vec.retain(|o| condition)` — keep only items where condition is true (like filter in-place)

---

### 3.6 `BTreeMap::entry()` — The Insert-or-Update Pattern

```rust
self.bids.entry(Price(order.price))
    .and_modify(|bids| bids.push(order.clone()))
    .or_insert_with(|| vec![order.clone()]);
```

This is the canonical Rust way to say: "If this key exists, run this code on the value. If it doesn't exist, insert this default value."

**Step by step:**
1. `.entry(Price(order.price))` — look up or prepare to insert this key
2. `.and_modify(|bids| bids.push(...))` — if key exists, add order to the Vec
3. `.or_insert_with(|| vec![...])` — if key doesn't exist, create a new Vec with this order

**Layman version:** Open the filing drawer labeled "$100." If the drawer already exists, drop the paper in. If it doesn't exist, make the drawer and put the paper in.

---

### 3.7 `retain()` — In-Place Filtering

```rust
self.asks.retain(|_price, asks| {
    asks.retain(|ask| ask.filled < ask.quantity);
    !asks.is_empty()
});
```

`retain` keeps only elements for which the closure returns `true`. Used here to:
1. Inner `retain`: remove fully-filled orders from each Vec
2. Outer `retain`: remove price levels whose Vec is now empty

**Layman version:** Go through every box on the shelf. Inside each box, throw out empty containers. Then if the box itself is empty, throw out the box too.

---

### 3.8 Closures — `|x| expression`

```rust
.map(|o| o.quantity - o.filled)
```

A closure is an anonymous function. `|o|` declares the parameter, `o.quantity - o.filled` is the body. Think of `|o| o.quantity - o.filled` as a mini-function you pass to `map`.

---

### 3.9 Iterator Combinators — `map`, `sum`, `flat_map`, `filter`, `find_map`

These chain together to process collections without explicit loops:

```rust
orders.iter()
    .map(|o| o.quantity - o.filled)   // transform each order into remaining quantity
    .sum::<f64>()                      // add them all up
```

Equivalent to:
```rust
let mut total = 0.0;
for o in orders.iter() {
    total += o.quantity - o.filled;
}
```

The iterator combinator version is idiomatic Rust — prefer it.

---

### 3.10 `clone()` — Making a Copy

`order.clone()` creates a deep copy of an order. Required when you need to store the order in the BTreeMap *and* keep the original. Rust won't let you move the same value to two places.

**Important:** `Clone` is derived via `#[derive(Clone)]` on the struct — Rust auto-generates the clone logic.

---

### 3.11 Arc, Mutex, Box — When You'd Use Them

This file doesn't use these, but a real exchange would:

```rust
// In a real multi-threaded exchange:
let orderbook: Arc<Mutex<Orderbook>> = Arc::new(Mutex::new(Orderbook::new("BTC_USDT".into())));

// Thread 1 (order ingestion):
let ob = orderbook.lock().unwrap();
ob.add_order(&mut order);

// Thread 2 (market data publisher):
let ob = orderbook.lock().unwrap();
let depth = ob.get_depth();
```

- `Arc<T>` — shared ownership across threads (reference-counted pointer)
- `Mutex<T>` — exclusive write access (only one thread writes at a time)
- `Box<T>` — heap allocation for large types (avoided here since Orderbook is always used by reference)

---

## 4. Dependency & Import Block

```rust
use serde::{Deserialize, Serialize};
use crate::redis::redis_manager::OrderSide;
use log::info;
use std::collections::{BTreeMap, HashMap};
use std::cmp::Ordering;
```

| Import | Why It's Here |
|---|---|
| `serde::{Deserialize, Serialize}` | Allows structs to be converted to/from JSON (for Redis, API responses) |
| `crate::redis::redis_manager::OrderSide` | The Buy/Sell enum lives in another module of this project |
| `log::info` | Structured logging — `info!("message")` emits to log backend |
| `BTreeMap` | Sorted map — core data structure for bids/asks |
| `HashMap` | Unsorted map — used for fast order_id lookups |
| `Ordering` | The enum `Less / Equal / Greater` returned by comparison functions |

The `#[derive(Debug, Clone, Serialize, Deserialize)]` macros on each struct auto-generate:
- `Debug` — lets you print structs with `{:?}`
- `Clone` — lets you call `.clone()` to copy
- `Serialize` / `Deserialize` — JSON conversion via serde

---

## 5. The Price Newtype

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price(f64);

impl Eq for Price {}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}
```

### Why All Four Trait Impls?

Rust's type system has a hierarchy: `Ord` requires `Eq`, `Eq` requires `PartialEq`, `Ord` implies `PartialOrd`. So you must implement all four for BTreeMap compatibility.

### `self.0`

In a tuple struct like `Price(f64)`, the inner value is accessed as `.0` (the first field, zero-indexed). So `self.0` is the actual `f64` price value.

### Copy vs Clone

`#[derive(Copy)]` means `Price` values are automatically duplicated on assignment instead of moved. This is efficient for small types like `Price(f64)` (just 8 bytes).

---

## 6. The Fill Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub qty: f64,
    pub price: f64,
    pub trade_id: i64,
    pub marker_order_id: String,
    pub other_user_id: String,
}
```

A `Fill` is generated every time two orders match. It is returned from `add_order` so the calling layer can:

- Record the trade in a database
- Update user balances
- Publish to market data feeds
- Generate trade confirmations

### Field Breakdown

| Field | Type | Purpose |
|---|---|---|
| `qty` | f64 | How many units traded |
| `price` | f64 | The price at which they traded (maker's price) |
| `trade_id` | i64 | Auto-incrementing global trade counter |
| `marker_order_id` | String | The resting order that was hit (the "maker") |
| `other_user_id` | String | The counterparty's user ID |

**Note:** `marker_order_id` refers to the resting order (the one already on the book), not the incoming order. This naming follows exchange convention where the resting side is the "market maker."

---

## 7. The Order Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub price: f64,
    pub quantity: f64,
    pub order_id: String,
    pub filled: f64,
    pub side: OrderSide,
    pub user_id: String,
}
```

### The `filled` Field Is the Key to Everything

`filled` starts at `0.0` when an order is placed. Every time part of the order is matched, `filled` increases. The remaining open quantity is always:

```
remaining = quantity - filled
```

When `remaining == 0.0`, the order is fully matched and removed from the book.

### State Machine of an Order

```
Created (filled=0)
    │
    ├── Partially matched → filled=N, N < quantity → stays on book
    │
    ├── Fully matched → filled=quantity → removed from book
    │
    └── Cancelled → removed from book, filled stays wherever it was
```

---

## 8. The Orderbook Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Orderbook {
    market: String,
    pub bids: BTreeMap<Price, Vec<Order>>,
    pub asks: BTreeMap<Price, Vec<Order>>,
    last_trade_id: i64,
    current_price: f64,
    orders: HashMap<String, Order>,
}
```

### Field-by-Field

**`market: String`**
The trading pair identifier: `"BTC_USDT"`, `"ETH_USDT"`, etc. Private — accessed via `ticker()`.

**`bids: BTreeMap<Price, Vec<Order>>`**
All open buy orders. Key is price (sorted ascending by BTreeMap default). The *highest* price is the best bid — accessed via `.keys().max()` or by iterating in reverse.

**`asks: BTreeMap<Price, Vec<Order>>`**
All open sell orders. Key is price (sorted ascending). The *lowest* price is the best ask — the first key in the map.

**`last_trade_id: i64`**
Monotonically increasing counter. Every Fill gets a unique `trade_id` by incrementing this. Private — ensures uniqueness within this orderbook.

**`current_price: f64`**
The last price at which a trade occurred. Currently not updated in the matching functions (a production gap). Used for reference price calculations.

**`orders: HashMap<String, Order>`**
A secondary index — allows O(1) lookup of any order by its `order_id`. Currently populated via `add_order` but notably absent in the cancel functions (another production gap).

### Why Two Structures (BTreeMap + HashMap)?

The BTreeMap is optimized for *price-priority matching* — finding the best price is O(log n).
The HashMap is optimized for *direct order lookup* — finding a specific order by ID is O(1).

They store duplicated data. In a production system you'd keep them in sync carefully.

---

## 9. The OrderbookSnapshot Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookSnapshot {
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
}
```

This is the **public view** of the orderbook — what you'd display in a trading UI or send over a websocket. Each entry is `(price_as_string, total_quantity_as_string)` — aggregated at each price level.

The use of `String` instead of `f64` is intentional for serialization safety — floating point JSON can have precision issues. Strings preserve exact representation.

---

## 10. Function: `new()`

```rust
pub fn new(market: String) -> Self {
    info!("Creating new orderbook for market: {}", market);
    Self {
        market,
        bids: BTreeMap::new(),
        asks: BTreeMap::new(),
        orders: HashMap::new(),
        last_trade_id: 0,
        current_price: 0.0,
    }
}
```

### What It Does

Constructor for `Orderbook`. Takes the market name and returns an empty orderbook with all collections initialized but empty.

### Rust Syntax Notes

- `Self` refers to the type being implemented (Orderbook) — avoids repeating the type name.
- `market` without `: market` in the struct literal is **shorthand field initialization** — Rust auto-assigns the variable `market` to the field `market` when names match.
- `info!(...)` writes a log line. In production this goes to stdout, a file, or a log aggregator.

### When Is This Called?

Once per trading pair at exchange startup, or when a new market is created dynamically.

---

## 11. Function: `ticker()`

```rust
pub fn ticker(&self) -> &str {
    &self.market
}
```

### What It Does

Returns a string reference to the market name. `&str` instead of `String` because we're borrowing the internal string — no allocation, no copy.

### Lifetime Note

The returned `&str` is valid only as long as the `Orderbook` exists. Rust enforces this automatically via lifetime rules. You can't store this reference after the Orderbook is dropped.

---

## 12. Function: `get_snapshot()`

```rust
#[allow(dead_code)]
pub fn get_snapshot(&self) -> Orderbook {
    let snapshot = Orderbook {
        market: self.market.clone(),
        bids: self.bids.clone(),
        asks: self.asks.clone(),
        orders: self.orders.clone(),
        last_trade_id: self.last_trade_id,
        current_price: self.current_price,
    };
    snapshot
}
```

### What It Does

Creates a **complete deep copy** of the entire orderbook at a moment in time. This is how you'd take a point-in-time snapshot for persistence, auditing, or recovery.

### `#[allow(dead_code)]`

This tells the Rust compiler "I know this function isn't called anywhere yet — don't warn me." It's a suppression attribute, useful for work-in-progress code.

### Deep Clone Cost

Cloning an entire orderbook is expensive — O(n) where n is total orders. In production you'd either:
- Use copy-on-write data structures
- Keep a separate snapshot buffer updated incrementally
- Serialize directly to Redis without cloning

---

## 13. Function: `add_order()`

```rust
pub fn add_order(&mut self, order: &mut Order) -> Result<(Vec<Fill>, f64), String> {
    if order.side == OrderSide::Buy {
        let (fills, executed_qty) = self.match_bid(order).expect("Error matching bid");
        order.filled = executed_qty;
        if executed_qty == order.quantity {
            Ok((fills, executed_qty))
        } else {
            self.bids.entry(Price(order.price))
                .and_modify(|bids| bids.push(order.clone()))
                .or_insert_with(|| vec![order.clone()]);
            Ok((fills, executed_qty))
        }
    } else {
        let (fills, executed_qty) = self.match_ask(order).expect("Error matching ask");
        order.filled = executed_qty;
        if executed_qty == order.quantity {
            Ok((fills, executed_qty))
        } else {
            self.asks.entry(Price(order.price))
                .and_modify(|asks| asks.push(order.clone()))
                .or_insert_with(|| vec![order.clone()]);
            Ok((fills, executed_qty))
        }
    }
}
```

### What It Does — The Full Flow

1. **Branch on side** — Buy goes to `match_bid`, Sell goes to `match_ask`
2. **Attempt matching** — Try to match against the opposite side of the book
3. **Update filled amount** — Record how much of the order was matched
4. **Conditional insertion:**
   - If fully matched: return fills, don't add to book
   - If partially/unmatched: add remaining order to the book as a resting order

### Why `.expect()` Instead of `?`

`expect` panics on error with the given message. This is used here instead of `?` because the match functions currently always return `Ok(...)` — the `Err` path is never reached. In production you'd use `?` to propagate errors properly.

### The `entry()` Pattern in Detail

```rust
self.bids.entry(Price(order.price))
    .and_modify(|bids| bids.push(order.clone()))
    .or_insert_with(|| vec![order.clone()]);
```

- **Scenario A (price already exists):** `entry()` finds the key, `.and_modify()` runs, pushing the order into the existing Vec. `.or_insert_with()` is skipped.
- **Scenario B (price is new):** `entry()` finds no key, `.and_modify()` is skipped, `.or_insert_with()` creates a new `vec![order.clone()]` and inserts it.

Note: `order.clone()` is called because `order` is borrowed (`&mut Order`) — we can't move it into the BTreeMap. The clone becomes the stored copy.

---

## 14. Function: `match_bid()`

```rust
pub fn match_bid(&mut self, order: &Order) -> Result<(Vec<Fill>, f64), String> {
    let mut fills = Vec::new();
    let mut executed_qty = 0.0;

    let mut price_levels: Vec<Price> = self.asks.keys().cloned().collect();
    price_levels.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    for price in price_levels {
        if price.0 <= order.price && executed_qty < order.quantity {
            if let Some(asks) = self.asks.get_mut(&price) {
                let remaining_to_fill = order.quantity - executed_qty;
                let mut filled_at_this_level = 0.0;

                for ask in asks.iter_mut() {
                    if ask.user_id != order.user_id {
                        let ask_remaining = ask.quantity - ask.filled;
                        if ask_remaining > 0.0 {
                            let fill_qty = f64::min(
                                remaining_to_fill - filled_at_this_level,
                                ask_remaining
                            );
                            if fill_qty > 0.0 {
                                ask.filled += fill_qty;
                                filled_at_this_level += fill_qty;
                                fills.push(Fill {
                                    price: price.0,
                                    qty: fill_qty,
                                    trade_id: {
                                        self.last_trade_id += 1;
                                        self.last_trade_id
                                    },
                                    other_user_id: ask.user_id.clone(),
                                    marker_order_id: ask.order_id.clone(),
                                });
                                if filled_at_this_level >= remaining_to_fill {
                                    break;
                                }
                            }
                        }
                    }
                }
                executed_qty += filled_at_this_level;
            }
        }
    }

    self.asks.retain(|_price, asks| {
        asks.retain(|ask| ask.filled < ask.quantity);
        !asks.is_empty()
    });

    Ok((fills, executed_qty))
}
```

### What It Does — Line by Line

**Initialization:**
```rust
let mut fills = Vec::new();      // empty list of trade receipts
let mut executed_qty = 0.0;      // total amount matched so far
```

**Why collect price levels into a separate Vec?**
```rust
let mut price_levels: Vec<Price> = self.asks.keys().cloned().collect();
```
This is a borrow checker necessity. If you iterated `self.asks` directly while also calling `self.asks.get_mut()`, Rust would see two borrows of `self.asks` simultaneously — one immutable (the iterator) and one mutable (`get_mut`). This is forbidden. Solution: collect the keys into an independent `Vec<Price>`, iterate that, then mutably access `self.asks` inside the loop.

**Sort ascending — cheapest ask first:**
```rust
price_levels.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
```
A buyer always pays the cheapest available price first. If asks are at $103, $104, $105, we process $103 first.

**Matching condition:**
```rust
if price.0 <= order.price && executed_qty < order.quantity
```
Two conditions must both be true:
1. The ask price is affordable (≤ buyer's limit price)
2. We still have quantity left to fill

**Self-trade prevention:**
```rust
if ask.user_id != order.user_id
```
You cannot be both buyer and seller in the same trade. This is a regulatory requirement on all exchanges.

**Fill quantity calculation:**
```rust
let fill_qty = f64::min(
    remaining_to_fill - filled_at_this_level,  // how much buyer still needs
    ask_remaining                               // how much this seller has
);
```
`f64::min` takes the smaller of: what the buyer still needs vs what this seller has. You can't fill more than either party has.

**Trade ID increment:**
```rust
trade_id: {
    self.last_trade_id += 1;
    self.last_trade_id
},
```
This Rust block syntax: the block evaluates to its last expression. So this increments `last_trade_id` and uses the new value as the `trade_id`. Guarantees each trade gets a unique, incrementing ID.

**Early exit:**
```rust
if filled_at_this_level >= remaining_to_fill {
    break;
}
```
If we've filled everything the buyer needs at this price level, stop looping through more orders at this price.

**Cleanup:**
```rust
self.asks.retain(|_price, asks| {
    asks.retain(|ask| ask.filled < ask.quantity);
    !asks.is_empty()
});
```
After matching, sweep through and remove:
1. Any ask where `filled >= quantity` (fully matched)
2. Any price level where all asks have been removed (empty Vec)

---

## 15. Function: `match_ask()`

This is the mirror of `match_bid`. The key differences:

| Aspect | match_bid (buying) | match_ask (selling) |
|---|---|---|
| Scans | asks (sellers) | bids (buyers) |
| Sort direction | Ascending (cheapest first) | Descending (most expensive first) |
| Price condition | `ask.price <= buyer.price` | `bid.price >= seller.price` |
| Cleanup target | self.asks | self.bids |

**Sort descending — highest bid first:**
```rust
price_levels.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
// Note: b.cmp(a) instead of a.cmp(b) — reversed!
```
A seller always wants the highest paying buyer first.

**Price condition:**
```rust
if price.0 >= order.price
```
The buyer's offered price must meet or exceed the seller's minimum asking price.

Everything else is structurally identical to `match_bid`.

---

## 16. Function: `get_depth()`

```rust
pub fn get_depth(&self) -> OrderbookSnapshot {
    let mut bids: Vec<(String, String)> = Vec::new();
    let mut asks: Vec<(String, String)> = Vec::new();

    for (price, orders) in &self.bids {
        let remaining = orders.iter().map(|o| o.quantity - o.filled).sum::<f64>();
        if remaining > 0.0 {
            bids.push((price.0.to_string(), remaining.to_string()));
        }
    }

    for (price, orders) in &self.asks {
        let remaining = orders.iter().map(|o| o.quantity - o.filled).sum::<f64>();
        if remaining > 0.0 {
            asks.push((price.0.to_string(), remaining.to_string()));
        }
    }

    bids.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap()); // descending
    asks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap()); // ascending

    OrderbookSnapshot { bids, asks }
}
```

### What It Does

Produces the aggregated market depth — what you see on any trading platform's Level 2 view:

```
BIDS                    ASKS
Price    Volume         Price    Volume
102.00   15.5          103.00   8.0
101.00   30.0          104.00   22.5
100.00   45.0          105.00   11.0
```

For each price level, it sums all remaining quantities across all individual orders:
```rust
let remaining = orders.iter()
    .map(|o| o.quantity - o.filled)   // remaining per order
    .sum::<f64>();                      // total remaining at this price
```

The `::<f64>` is a **turbofish** — explicitly telling Rust what type `sum` should produce, because the compiler can't always infer it from context alone.

### Why Sort Again?

The BTreeMap stores prices in ascending order. For the snapshot:
- Bids should be descending (highest first — best bid on top)
- Asks should be ascending (lowest first — best ask on top)

So bids need to be reversed via a sort, asks are already in the right order but sorted again for consistency.

---

## 17. Function: `get_open_orders()`

```rust
pub fn get_open_orders(&self, user_id: &str) -> Vec<Order> {
    let mut orders = Vec::new();
    orders.extend(
        self.bids.values()
            .flat_map(|bids| bids.iter()
                .filter(|o| o.user_id == user_id && o.filled < o.quantity))
            .cloned(),
    );
    orders.extend(
        self.asks.values()
            .flat_map(|asks| asks.iter()
                .filter(|o| o.user_id == user_id && o.filled < o.quantity))
            .cloned(),
    );
    orders
}
```

### What It Does

Returns all open (not fully filled) orders for a specific user. Used for the "My Open Orders" view in a trading UI.

### Iterator Chain Breakdown

```rust
self.bids.values()                                    // iterate Vec<Order> at each price
    .flat_map(|bids| bids.iter()                      // flatten: Vec<Vec<Order>> → Vec<Order>
        .filter(|o| o.user_id == user_id              // only this user's orders
             && o.filled < o.quantity))               // only unfilled orders
    .cloned()                                         // convert &Order to Order (owned copy)
```

`flat_map` is `map` + `flatten` combined. Without it, you'd get an iterator of iterators (each Vec produces its own iterator). `flat_map` collapses this into one flat iterator.

### Complexity

This is O(n) where n is total orders in the book — it scans everything. In production with millions of orders, you'd maintain a secondary index: `user_orders: HashMap<String, Vec<OrderId>>`.

---

## 18. Function: `cancel_bid()`

```rust
pub fn cancel_bid(&mut self, order_id: &str) -> Result<f64, String> {
    let price = self.bids.iter()
        .find_map(|(price, bids)| 
            bids.iter()
                .position(|bid| bid.order_id == order_id)
                .map(|_| price.0))
        .ok_or("Order not found")?;

    self.bids.entry(Price(price)).and_modify(|bids| {
        bids.retain(|bid| bid.order_id != order_id);
    });
    Ok(price)
}
```

### What It Does

Removes a specific bid from the book and returns the price level it was at. The returned price allows the caller to update any price-level displays.

### The `find_map` Pattern

```rust
.find_map(|(price, bids)|
    bids.iter()
        .position(|bid| bid.order_id == order_id)
        .map(|_| price.0))
```

This is elegant but dense. Breaking it apart:
1. `self.bids.iter()` — iterate over all price levels
2. For each price level, `bids.iter().position(|bid| bid.order_id == order_id)` — returns `Some(index)` if found, `None` if not
3. `.map(|_| price.0)` — if found (Some), discard the index and return the price instead
4. `find_map` stops at the first `Some` and returns it, or returns `None` if nothing matches

We don't need the index (position) to remove — we use `retain` instead. We only need the price to look up the right BTreeMap entry.

### `.ok_or("Order not found")?`

Converts `Option<f64>` to `Result<f64, String>`. If `find_map` returned `None` (order not found), `.ok_or("Order not found")` turns it into `Err("Order not found")`. The `?` then immediately returns that error from `cancel_bid`.

### The Remove Step

```rust
self.bids.entry(Price(price)).and_modify(|bids| {
    bids.retain(|bid| bid.order_id != order_id);
});
```

Gets the Vec at this price level and uses `retain` to remove the matching order. After this, the Vec contains all orders *except* the cancelled one.

**Production gap:** The price level's Vec might now be empty, but we don't remove the empty Vec from the BTreeMap. This is a minor memory leak.

---

## 19. Function: `cancel_ask()`

Identical logic to `cancel_bid`, operating on `self.asks` instead of `self.bids`. Returns the price level of the cancelled ask.

---

## 20. The Test Suite — Every Test Explained

### Test 1: `test_add_bid_order`

```rust
fn test_add_bid_order() {
    let mut orderbook = Orderbook::new("TEST_MARKET".to_string());
    let mut order = Order { side: OrderSide::Buy, price: 100.0, quantity: 10.0, ... };
    let (fills, executed_qty) = orderbook.add_order(&mut order).unwrap();
    assert_eq!(fills.len(), 0);     // no match (book was empty)
    assert_eq!(executed_qty, 0.0);  // nothing executed
    assert_eq!(orderbook.bids.len(), 1); // one price level in bids
    assert_eq!(orderbook.asks.len(), 0); // asks still empty
}
```

**Tests:** That a buy order with no matching asks goes straight to the book.

---

### Test 2: `test_add_ask_order`

Mirror of Test 1. Sell order with no matching bids goes to the asks side.

---

### Test 3: `test_match_orders_different_users`

```rust
// Add buy order from user1 at $100 for qty 5
// Add sell order from user2 at $100 for qty 5
// Expect: 1 fill, 5.0 executed, both sides empty (fully matched)
assert_eq!(fills.len(), 1);
assert_eq!(executed_qty, 5.0);
assert_eq!(orderbook.bids.len(), 0);
assert_eq!(orderbook.asks.len(), 0);
```

**Tests:** The core matching functionality — two different users, matching prices, both fully filled.

---

### Test 4: `test_match_orders_same_user`

```rust
// user1 places a buy order
// user1 places a sell order at the same price
// Expect: 0 fills (self-trade prevention)
assert_eq!(fills.len(), 0);
assert_eq!(executed_qty, 0.0);
assert_eq!(orderbook.bids.len(), 1);  // buy still on book
assert_eq!(orderbook.asks.len(), 1);  // sell added to book too
```

**Tests:** Self-trade prevention — the same user cannot trade with themselves.

---

### Test 5: `test_partial_match`

```rust
// Buy order: qty 10 at $100
// Sell order: qty 5 at $100
// Expect: sell fully matched, buy has 5 remaining
assert_eq!(fills.len(), 1);
assert_eq!(executed_qty, 5.0);      // sell was qty 5
assert_eq!(orderbook.bids.len(), 1); // buy still on book (partial)
assert_eq!(orderbook.asks.len(), 0); // sell fully matched, removed
let remaining_qty = bids[0].quantity - bids[0].filled;
assert_eq!(remaining_qty, 5.0);      // 10 - 5 = 5 remaining
```

**Tests:** That a partially-filled order stays on the book with the correct remaining quantity.

---

### Test 6: `test_price_priority`

```rust
// Buy order 1: qty 5 at $100 from user1
// Buy order 2: qty 5 at $102 from user2
// Sell order: qty 5 at $99 from user3
// Expect: sell matches with the HIGHER buy ($102), not $100
assert_eq!(fills[0].price, 102.0);
assert_eq!(orderbook.bids.len(), 1); // only $100 order remains
```

**Tests:** Price-time priority — the best price is matched first. A seller gets the highest paying buyer. This is the foundational rule of all exchange matching.

---

## 21. Data Flow — End to End

Here is the complete journey of an order through the system:

```
External caller
    │
    │  Order { user_id: "alice", side: Buy, price: 103.0, quantity: 10.0, filled: 0.0, ... }
    │
    ▼
add_order(&mut order)
    │
    ├─[Buy]──► match_bid(&order)
    │              │
    │              ├── Collect ask price levels: [103.0, 104.0, 105.0]
    │              ├── Sort ascending: [103.0, 104.0, 105.0]
    │              │
    │              ├── Level 103.0: price(103.0) <= order.price(103.0)? YES
    │              │       ├── Ask from "bob": qty=7, filled=0
    │              │       │       user_id check: "bob" != "alice"? YES
    │              │       │       ask_remaining = 7 - 0 = 7
    │              │       │       fill_qty = min(10-0, 7) = 7
    │              │       │       bob.filled += 7  → bob.filled = 7
    │              │       │       fills.push(Fill { qty:7, price:103.0, trade_id:1, ... })
    │              │       │       filled_at_level = 7
    │              │       │       7 < 10 → continue to next ask at this level
    │              │       │
    │              │       └── Ask from "charlie": qty=5, filled=0
    │              │               fill_qty = min(10-7, 5) = min(3, 5) = 3
    │              │               charlie.filled += 3
    │              │               fills.push(Fill { qty:3, price:103.0, trade_id:2, ... })
    │              │               filled_at_level = 10  ← equals remaining_to_fill
    │              │               BREAK — price level done
    │              │
    │              ├── executed_qty = 10
    │              ├── Clean up: bob fully filled → removed. charlie partial → stays.
    │              └── Return Ok(([Fill{7}, Fill{3}], 10.0))
    │
    ├── order.filled = 10.0
    ├── executed_qty(10.0) == order.quantity(10.0) → fully filled
    └── Return Ok(([Fill{7}, Fill{3}], 10.0)) to caller
        │
        ▼
    Caller records 2 fills:
    - Alice bought 7 BTC from Bob @ $103
    - Alice bought 3 BTC from Charlie @ $103
    Alice's balance: -$1030 BTC, +10 BTC
    Bob's balance: +$721, -7 BTC
    Charlie's balance: +$309, -3 BTC (2 BTC still resting on book)
```

---

## 22. Common Patterns Used in This File

### Pattern 1: Collect-Sort-Iterate (Borrow Checker Workaround)

```rust
let mut price_levels: Vec<Price> = self.asks.keys().cloned().collect();
price_levels.sort_by(...);
for price in price_levels {
    if let Some(asks) = self.asks.get_mut(&price) { ... }
}
```

**Why:** Can't mutably borrow `self.asks` inside a loop that's iterating `self.asks`. Collecting keys into a separate `Vec` breaks this conflict.

### Pattern 2: Block Expression for Side Effects + Value

```rust
trade_id: {
    self.last_trade_id += 1;
    self.last_trade_id
},
```

**Why:** Rust blocks return their last expression. This pattern increments and returns in one place, inside a struct literal.

### Pattern 3: Option → Result Conversion

```rust
.ok_or("Order not found")?
```

**Why:** `find_map` returns `Option<T>`. Functions return `Result<T, E>`. `.ok_or(err)` bridges them. The `?` propagates the error.

### Pattern 4: Double-Layer `retain` for Nested Cleanup

```rust
self.bids.retain(|_price, bids| {
    bids.retain(|bid| bid.filled < bid.quantity);
    !bids.is_empty()
});
```

**Why:** Clean both levels at once. Inner retain removes completed orders. Outer retain removes empty price buckets. All in-place, no allocation.

### Pattern 5: `entry().and_modify().or_insert_with()`

```rust
self.bids.entry(Price(order.price))
    .and_modify(|v| v.push(order.clone()))
    .or_insert_with(|| vec![order.clone()]);
```

**Why:** Rust's idiomatic map insert-or-update. No if-else, no double lookup.

---

## 23. Edge Cases & Gotchas

### Float Precision

```rust
if executed_qty == order.quantity { ... }
```

This uses `==` on `f64`, which can fail for non-integer quantities due to floating point representation. For example, `0.1 + 0.2 != 0.3` in IEEE 754 arithmetic. The tests use clean integers (5.0, 10.0) so this works, but production code should use:

```rust
(executed_qty - order.quantity).abs() < 1e-9
```

### Self-Trade Not Fully Guarded in get_depth()

`get_depth()` shows all orders including those from the same user. If Alice has both a bid and ask open, both appear in the depth. This is correct behavior.

### Empty Vec After Cancel

`cancel_bid`/`cancel_ask` don't clean up empty price-level Vecs from the BTreeMap. After cancelling the only order at a price level, an empty `Vec<Order>` remains mapped to that price key. Over time with many cancellations this wastes memory.

### Borrow Checker vs. Trade ID

The trade_id increment is written as a block expression inside a struct literal:
```rust
trade_id: {
    self.last_trade_id += 1;
    self.last_trade_id
},
```
This is necessary because Rust evaluates struct field expressions in order, and `self` is mutably borrowed for the entire struct literal construction. This approach works but is somewhat unusual.

### `orders` HashMap Never Queried

The `orders: HashMap<String, Order>` field is declared and initialized but never populated or queried in any function. It's a placeholder for a future "get order by ID" O(1) lookup feature.

---

## 24. What This File Is Missing (Production Gaps)

| Missing Feature | Description | Real-World Impact |
|---|---|---|
| Market orders | Orders that execute at any price | Most common order type |
| Stop-loss orders | Triggered when price reaches a level | Essential for risk management |
| `current_price` update | Never updated after trades | Last price display is always 0.0 |
| `orders` HashMap population | Declared but never filled | Cannot look up order by ID in O(1) |
| Empty Vec cleanup in cancel | Memory leak on cancel | Grows unbounded over time |
| Thread safety | No Arc/Mutex | Crashes on concurrent access |
| Tick size enforcement | No minimum price increment | Allows $100.000001 orders |
| Lot size enforcement | No minimum quantity increment | Allows 0.000001 BTC orders |
| Order expiry (GTC, IOC, FOK) | All orders rest until filled/cancelled | No time-in-force support |
| Audit log | No trade history stored | Cannot replay events |
| Float-safe comparison | Uses `==` on f64 | Will fail with fractional quantities |

---

## 25. The 100-Step Build Plan

This is the exact sequence to rebuild this file from zero, in a way that always compiles and always has passing tests.

---

### Phase 1: Project Setup (Steps 1–10)

**Step 1.** Create a new Rust library:
```bash
cargo new --lib exchange_engine
cd exchange_engine
```

**Step 2.** Add dependencies to `Cargo.toml`:
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
log = "0.4"
rand = "0.8"

[dev-dependencies]
rand = "0.8"
```

**Step 3.** Create the file structure:
```
src/
  lib.rs
  trade/
    mod.rs
    orderbook.rs
  redis/
    mod.rs
    redis_manager.rs
```

**Step 4.** In `src/redis/redis_manager.rs`, define the `OrderSide` enum:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}
```

**Step 5.** In `src/redis/mod.rs`:
```rust
pub mod redis_manager;
```

**Step 6.** In `src/trade/mod.rs`:
```rust
pub mod orderbook;
```

**Step 7.** In `src/lib.rs`:
```rust
pub mod redis;
pub mod trade;
```

**Step 8.** Verify the project builds with no code yet:
```bash
cargo build
```

**Step 9.** Create `src/trade/orderbook.rs` as an empty file. Add the import block at the top:
```rust
use serde::{Deserialize, Serialize};
use crate::redis::redis_manager::OrderSide;
use log::info;
use std::collections::{BTreeMap, HashMap};
use std::cmp::Ordering;
```

**Step 10.** Run `cargo build` — it should compile (empty file with imports).

---

### Phase 2: The Price Newtype (Steps 11–20)

**Step 11.** Define the `Price` tuple struct:
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Price(f64);
```

**Step 12.** Try `cargo build`. It will fail: BTreeMap requires `Ord`. This is intentional — you're learning what the compiler demands.

**Step 13.** Add `PartialEq` derive:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price(f64);
```

**Step 14.** Implement `Eq` (marker trait — empty body):
```rust
impl Eq for Price {}
```

**Step 15.** Implement `PartialOrd`. Access the inner f64 with `.0`:
```rust
impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
```

**Step 16.** Implement `Ord`. Call `partial_cmp` and handle NaN with `.unwrap_or`:
```rust
impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}
```

**Step 17.** Run `cargo build`. Should compile now.

**Step 18.** Write a quick inline test to verify Price ordering:
```rust
#[test]
fn test_price_ordering() {
    assert!(Price(100.0) < Price(101.0));
    assert!(Price(100.0) == Price(100.0));
    assert!(Price(102.0) > Price(100.0));
}
```

**Step 19.** Run `cargo test`. Price tests should pass.

**Step 20.** Remove the inline test (it was for learning). Keep the impl blocks.

---

### Phase 3: Define All Structs (Steps 21–35)

**Step 21.** Define the `Fill` struct:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub qty: f64,
    pub price: f64,
    pub trade_id: i64,
    pub marker_order_id: String,
    pub other_user_id: String,
}
```

**Step 22.** Define the `Order` struct:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub price: f64,
    pub quantity: f64,
    pub order_id: String,
    pub filled: f64,
    pub side: OrderSide,
    pub user_id: String,
}
```

**Step 23.** Define the `Orderbook` struct:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Orderbook {
    market: String,
    pub bids: BTreeMap<Price, Vec<Order>>,
    pub asks: BTreeMap<Price, Vec<Order>>,
    last_trade_id: i64,
    current_price: f64,
    orders: HashMap<String, Order>,
}
```

**Step 24.** Define the `OrderbookSnapshot` struct:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookSnapshot {
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
}
```

**Step 25.** Run `cargo build`. All structs should compile cleanly.

**Step 26.** Verify you can instantiate an Order manually in a test:
```rust
let order = Order {
    order_id: "test-001".to_string(),
    user_id: "alice".to_string(),
    price: 100.0,
    quantity: 10.0,
    filled: 0.0,
    side: OrderSide::Buy,
};
println!("{:?}", order);
```

**Step 27.** Verify you can instantiate a Fill:
```rust
let fill = Fill {
    qty: 5.0, price: 100.0, trade_id: 1,
    marker_order_id: "test-001".to_string(),
    other_user_id: "bob".to_string(),
};
```

**Step 28.** Verify you can instantiate an Orderbook:
```rust
let ob = Orderbook {
    market: "BTC_USDT".to_string(),
    bids: BTreeMap::new(),
    asks: BTreeMap::new(),
    orders: HashMap::new(),
    last_trade_id: 0,
    current_price: 0.0,
};
```

**Step 29.** You now have all data types. Run `cargo build`. Clean compile.

**Step 30.** Mentally quiz yourself: What is the remaining quantity of an order? Answer: `order.quantity - order.filled`.

**Step 31.** Mentally quiz yourself: What does a BTreeMap key of `Price(100.0)` map to? Answer: `Vec<Order>` — all orders at that price.

**Step 32.** Mentally quiz yourself: What type does `add_order` return? Answer: `Result<(Vec<Fill>, f64), String>` — a list of fills and the executed quantity, or an error string.

**Step 33.** Commit your progress (if using git):
```bash
git add . && git commit -m "Phase 3: all structs defined"
```

**Step 34.** Read the test module imports at the bottom of the original file. Notice they import `rand` for generating order IDs:
```rust
use rand::thread_rng;
use rand::distributions::{Alphanumeric, DistString};
fn generate_order_id() -> String {
    Alphanumeric.sample_string(&mut thread_rng(), 24)
}
```

**Step 35.** Add the test module skeleton (empty for now):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;
    use rand::distributions::{Alphanumeric, DistString};
    fn generate_order_id() -> String {
        Alphanumeric.sample_string(&mut thread_rng(), 24)
    }
}
```

---

### Phase 4: Implement the `impl Orderbook` Block (Steps 36–50)

**Step 36.** Open the impl block:
```rust
impl Orderbook {
    // functions go here
}
```

**Step 37.** Implement `new()`:
```rust
pub fn new(market: String) -> Self {
    info!("Creating new orderbook for market: {}", market);
    Self {
        market,
        bids: BTreeMap::new(),
        asks: BTreeMap::new(),
        orders: HashMap::new(),
        last_trade_id: 0,
        current_price: 0.0,
    }
}
```

**Step 38.** Run `cargo build`. Verify `Orderbook::new("BTC_USDT".to_string())` compiles.

**Step 39.** Implement `ticker()`:
```rust
pub fn ticker(&self) -> &str {
    &self.market
}
```

**Step 40.** Implement `get_snapshot()`:
```rust
#[allow(dead_code)]
pub fn get_snapshot(&self) -> Orderbook {
    Orderbook {
        market: self.market.clone(),
        bids: self.bids.clone(),
        asks: self.asks.clone(),
        orders: self.orders.clone(),
        last_trade_id: self.last_trade_id,
        current_price: self.current_price,
    }
}
```

**Step 41.** Write `test_add_bid_order` (but don't implement `add_order` yet — just the test skeleton):
```rust
#[test]
fn test_add_bid_order() {
    let mut orderbook = Orderbook::new("TEST_MARKET".to_string());
    // Will call add_order once implemented
}
```

**Step 42.** Begin `add_order` — write only the function signature and an empty body:
```rust
pub fn add_order(&mut self, order: &mut Order) -> Result<(Vec<Fill>, f64), String> {
    todo!() // placeholder — compile but don't run
}
```

**Step 43.** Run `cargo build`. Verify it compiles with `todo!()`.

**Step 44.** Fill in the Buy branch of `add_order`, but call a stub `match_bid` that returns empty:
```rust
if order.side == OrderSide::Buy {
    let fills: Vec<Fill> = vec![];
    let executed_qty = 0.0;
    order.filled = executed_qty;
    self.bids.entry(Price(order.price))
        .and_modify(|bids| bids.push(order.clone()))
        .or_insert_with(|| vec![order.clone()]);
    Ok((fills, executed_qty))
} else {
    todo!()
}
```

**Step 45.** Fill in `test_add_bid_order` and run it:
```rust
let mut order = Order {
    order_id: generate_order_id(),
    user_id: "user1".to_string(),
    price: 100.0,
    quantity: 10.0,
    filled: 0.0,
    side: OrderSide::Buy,
};
let (fills, executed_qty) = orderbook.add_order(&mut order).unwrap();
assert_eq!(fills.len(), 0);
assert_eq!(executed_qty, 0.0);
assert_eq!(orderbook.bids.len(), 1);
assert_eq!(orderbook.asks.len(), 0);
```

**Step 46.** Run `cargo test test_add_bid_order`. It should pass.

**Step 47.** Do the same for the Sell branch and `test_add_ask_order`.

**Step 48.** Now both basic insertion tests pass. Commit.

**Step 49.** Write the `test_match_orders_different_users` test (it will fail until we implement matching — that's expected).

**Step 50.** Confirm the test fails with the right error: `assertion failed: fills.len() == 1`. Good — expected.

---

### Phase 5: Implement `match_bid()` (Steps 51–65)

**Step 51.** Write the `match_bid` function signature:
```rust
pub fn match_bid(&mut self, order: &Order) -> Result<(Vec<Fill>, f64), String> {
    todo!()
}
```

**Step 52.** Initialize the accumulators:
```rust
let mut fills = Vec::new();
let mut executed_qty = 0.0;
```

**Step 53.** Collect price levels from asks (solve the borrow problem):
```rust
let mut price_levels: Vec<Price> = self.asks.keys().cloned().collect();
```

**Step 54.** Sort ascending — cheapest ask first:
```rust
price_levels.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
```

**Step 55.** Write the outer loop:
```rust
for price in price_levels {
    if price.0 <= order.price && executed_qty < order.quantity {
        // matching logic will go here
    }
}
```

**Step 56.** Get mutable access to asks at this price:
```rust
if let Some(asks) = self.asks.get_mut(&price) {
    let remaining_to_fill = order.quantity - executed_qty;
    let mut filled_at_this_level = 0.0;
```

**Step 57.** Write the inner loop over individual asks:
```rust
for ask in asks.iter_mut() {
    if ask.user_id != order.user_id {
        let ask_remaining = ask.quantity - ask.filled;
        if ask_remaining > 0.0 {
            // fill logic here
        }
    }
}
```

**Step 58.** Calculate fill quantity:
```rust
let fill_qty = f64::min(
    remaining_to_fill - filled_at_this_level,
    ask_remaining
);
```

**Step 59.** Guard against zero fill (floating point safety):
```rust
if fill_qty > 0.0 {
    ask.filled += fill_qty;
    filled_at_this_level += fill_qty;
```

**Step 60.** Create the Fill struct with the block expression for trade_id:
```rust
fills.push(Fill {
    price: price.0,
    qty: fill_qty,
    trade_id: {
        self.last_trade_id += 1;
        self.last_trade_id
    },
    other_user_id: ask.user_id.clone(),
    marker_order_id: ask.order_id.clone(),
});
```

**Step 61.** Add the early exit:
```rust
if filled_at_this_level >= remaining_to_fill {
    break;
}
```

**Step 62.** After the inner loop, accumulate executed quantity:
```rust
executed_qty += filled_at_this_level;
```

**Step 63.** After the outer loop, clean up fully-matched asks:
```rust
self.asks.retain(|_price, asks| {
    asks.retain(|ask| ask.filled < ask.quantity);
    !asks.is_empty()
});
```

**Step 64.** Return the result:
```rust
Ok((fills, executed_qty))
```

**Step 65.** Wire `match_bid` into `add_order`'s Buy branch:
```rust
let (fills, executed_qty) = self.match_bid(order).expect("Error matching bid");
```
Run `cargo test`. `test_match_orders_different_users` should now pass.

---

### Phase 6: Implement `match_ask()` (Steps 66–72)

**Step 66.** Copy the structure of `match_bid`. Change all references from `asks` to `bids`.

**Step 67.** Change sort direction — descending (highest bid first):
```rust
price_levels.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
// Note b before a — reversed!
```

**Step 68.** Change matching condition — bid price must be >= seller's ask:
```rust
if price.0 >= order.price && executed_qty < order.quantity {
```

**Step 69.** Change cleanup to operate on `self.bids`:
```rust
self.bids.retain(|_price, bids| {
    bids.retain(|bid| bid.filled < bid.quantity);
    !bids.is_empty()
});
```

**Step 70.** Wire `match_ask` into `add_order`'s Sell branch.

**Step 71.** Run `cargo test`. All tests should now pass.

**Step 72.** Run `cargo test -- --nocapture` to see `info!` log output. You'll see depth logs.

---

### Phase 7: Implement `get_depth()` (Steps 73–78)

**Step 73.** Write the function signature:
```rust
pub fn get_depth(&self) -> OrderbookSnapshot {
```

**Step 74.** Initialize empty Vecs:
```rust
let mut bids: Vec<(String, String)> = Vec::new();
let mut asks: Vec<(String, String)> = Vec::new();
```

**Step 75.** Aggregate bids — sum remaining quantities per price level:
```rust
for (price, orders) in &self.bids {
    let remaining = orders.iter().map(|o| o.quantity - o.filled).sum::<f64>();
    if remaining > 0.0 {
        bids.push((price.0.to_string(), remaining.to_string()));
    }
}
```

**Step 76.** Do the same for asks.

**Step 77.** Sort: bids descending, asks ascending. Return the snapshot:
```rust
bids.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
asks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
OrderbookSnapshot { bids, asks }
```

**Step 78.** Run `cargo build`. Verify.

---

### Phase 8: Implement `get_open_orders()` (Steps 79–83)

**Step 79.** Write the signature:
```rust
pub fn get_open_orders(&self, user_id: &str) -> Vec<Order> {
    let mut orders = Vec::new();
```

**Step 80.** Extend with bids — filter by user and unfilled:
```rust
orders.extend(
    self.bids.values()
        .flat_map(|bids| bids.iter()
            .filter(|o| o.user_id == user_id && o.filled < o.quantity))
        .cloned(),
);
```

**Step 81.** Repeat for asks.

**Step 82.** Return the Vec:
```rust
orders
```

**Step 83.** Run `cargo build`. Verify.

---

### Phase 9: Implement `cancel_bid()` and `cancel_ask()` (Steps 84–90)

**Step 84.** Write `cancel_bid` signature:
```rust
pub fn cancel_bid(&mut self, order_id: &str) -> Result<f64, String> {
```

**Step 85.** Find the price level using `find_map`:
```rust
let price = self.bids.iter()
    .find_map(|(price, bids)|
        bids.iter()
            .position(|bid| bid.order_id == order_id)
            .map(|_| price.0))
    .ok_or("Order not found")?;
```

**Step 86.** Remove the order using `retain`:
```rust
self.bids.entry(Price(price)).and_modify(|bids| {
    bids.retain(|bid| bid.order_id != order_id);
});
Ok(price)
```

**Step 87.** Implement `cancel_ask` — identical but for `self.asks`.

**Step 88.** Run `cargo build`. Verify.

**Step 89.** Write a manual test: add an order, cancel it, verify bids/asks length changed:
```rust
let mut ob = Orderbook::new("TEST".to_string());
let mut o = Order { order_id: "abc".to_string(), ...(fill in fields) };
ob.add_order(&mut o).unwrap();
assert_eq!(ob.bids.len(), 1);
ob.cancel_bid("abc").unwrap();
assert_eq!(ob.bids.len(), 0);
```

**Step 90.** Run all tests: `cargo test`. All 6 should pass.

---

### Phase 10: Polish and Validation (Steps 91–100)

**Step 91.** Run `cargo clippy` — Rust's linter. Fix any warnings.

**Step 92.** Run `cargo fmt` — auto-format all code to Rust style conventions.

**Step 93.** Add `#[allow(dead_code)]` to `get_snapshot` since it's intentionally unused.

**Step 94.** Review every `expect()` call. Add descriptive messages if missing:
```rust
.expect("Error matching bid: match_bid should never fail")
```

**Step 95.** Add doc comments to each public function:
```rust
/// Adds a new order to the orderbook and attempts to match it.
/// Returns a list of fills and the total executed quantity.
pub fn add_order(...)
```

**Step 96.** Verify all 6 original tests pass exactly:
- `test_add_bid_order`
- `test_add_ask_order`
- `test_match_orders_different_users`
- `test_match_orders_same_user`
- `test_partial_match`
- `test_price_priority`

**Step 97.** Write one additional test yourself: what happens when you place 3 buy orders at the same price and 1 sell order? Verify time-priority (first order in gets matched first).

**Step 98.** Stress test mentally: 2 buys at $100, 1 buy at $102. 1 sell at $99. Which buy matches? ($102 — price priority. If equal price, the one inserted first — time priority.)

**Step 99.** Review the production gaps section (Section 24). Pick one to implement as a stretch goal: try updating `current_price` after each fill, or cleaning up empty Vecs in cancel functions.

**Step 100.** Final check: `cargo test && cargo build --release`. Both should succeed with zero warnings.

---

## Appendix A: Quick Reference Card

```
BTreeMap<Price, Vec<Order>>
  │
  ├── Key: Price(f64) — sorted automatically
  │     └── implements Eq, PartialEq, Ord, PartialOrd manually
  │
  └── Value: Vec<Order> — all orders at that price (time-priority)
        └── Order { price, quantity, filled, order_id, user_id, side }
              └── remaining = quantity - filled

add_order(order) → Result<(Vec<Fill>, f64), String>
  ├── Buy → match_bid() → scan asks low→high → fill if ask.price ≤ bid.price
  └── Sell → match_ask() → scan bids high→low → fill if bid.price ≥ ask.price

Fill { qty, price, trade_id, marker_order_id, other_user_id }
  └── One per matched pair. Returned to caller for settlement.

Cleanup after matching:
  retain(|order| order.filled < order.quantity) → remove completed orders
  retain(|vec| !vec.is_empty())                 → remove empty price levels
```

---

## Appendix B: Trait Implementation Checklist for Price

When using any newtype as a `BTreeMap` key, implement in this order:

- [ ] `PartialEq` (can derive)
- [ ] `Eq` (empty impl, just a marker)
- [ ] `PartialOrd` (implement `partial_cmp`)
- [ ] `Ord` (implement `cmp`, call `partial_cmp`, handle NaN)

---

## Appendix C: Borrow Checker Patterns

| Problem | Solution Used |
|---|---|
| Can't iterate + mutate same BTreeMap | Collect keys into Vec, iterate Vec |
| Can't move Order into two places | Use `order.clone()` |
| Option → Result conversion | `.ok_or("message")?` |
| Increment + use in struct literal | `{ self.x += 1; self.x }` block |
| Insert or update a map entry | `.entry(k).and_modify().or_insert_with()` |
| Remove elements while iterating | `.retain(|x| keep_condition)` |

---

*End of document. Total approximate line count: 1050+.*
*Build this file once, understand it twice, and you'll have mastered a significant slice of real-world Rust.*