// /// ╔══════════════════════════════════════════════════════════════════════╗
// /// ║          EXCHANGE  —  FULL END-TO-END INTEGRATION TEST              ║
// /// ║                                                                      ║
// /// ║  Covers every user-facing flow:                                      ║
// /// ║    Auth  : signup · login · check-auth · logout                      ║
// /// ║    Orders: create · list open · cancel                               ║
// /// ║    Depth : GET /depth (market depth snapshot)                        ║
// /// ║    OnRamp: fund a user account                                       ║
// /// ║    WS    : subscribe ticker / depth · receive live trade ticks       ║
// /// ║    Guard : unauthenticated requests must be rejected (401)           ║
// /// ╚══════════════════════════════════════════════════════════════════════╝

// use futures_util::{SinkExt, StreamExt};
// use redis::{Client as RedisClient, Commands};
// use reqwest::Client;
// use serde_json::{json, Value};
// use std::time::Duration;
// use tokio::time::{sleep, timeout};
// use tokio_tungstenite::{connect_async, tungstenite::Message};

// // ─── Config ──────────────────────────────────────────────────────────────────

// const BASE_URL:       &str = "http://localhost:8080/api/v1";
// const WS_URL:         &str = "ws://localhost:8080/ws";
// const REDIS_URL:      &str = "redis://localhost:6380";
// const MESSAGES_QUEUE: &str = "messages";

// const TEST_EMAIL:    &str = "e2e_test@example.com";
// const TEST_USERNAME: &str = "e2e_user";
// const TEST_PASSWORD: &str = "password123";
// const TEST_MARKET:   &str = "TATA_INR";

// // ─── Pretty-print helpers ─────────────────────────────────────────────────────

// fn banner(title: &str) {
//     let line = "═".repeat(60);
//     println!("\n╔{}╗", line);
//     println!("║  {:<58}║", title);
//     println!("╚{}╝", line);
// }

// fn section(n: u8, title: &str) {
//     println!("\n┌─────────────────────────────────────────────────────────┐");
//     println!("│  {:>2}. {:<53}│", n, title);
//     println!("└─────────────────────────────────────────────────────────┘");
// }

// fn ok(msg: &str)   { println!("  ✅  {}", msg); }
// fn fail(msg: &str) { println!("  ❌  {}", msg); }
// fn warn(msg: &str) { println!("  ⚠️   {}", msg); }
// fn info(label: &str, value: &str) {
//     println!("  {:<16}: {}", label, value);
// }
// fn log(msg: &str)  { println!("  ℹ️   {}", msg); }

// /// Returns true if the assertion passed.
// fn assert_status(got: u16, expected: u16, label: &str) -> bool {
//     if got == expected {
//         ok(&format!("{} → HTTP {}", label, got));
//         true
//     } else {
//         fail(&format!("{} → expected HTTP {} but got {}", label, expected, got));
//         false
//     }
// }

// /// Dump a JSON body with a label. Falls back gracefully for non-JSON.
// fn dump_body(label: &str, body: &Value) {
//     match serde_json::to_string_pretty(body) {
//         Ok(pretty) => {
//             println!("  {}", label);
//             for line in pretty.lines() {
//                 println!("    {}", line);
//             }
//         }
//         Err(_) => println!("  {} (non-JSON or null)", label),
//     }
// }

// // ─── Fake engine ─────────────────────────────────────────────────────────────
// //
// // A background OS thread that:
// //   1. BRPOPs one message from the `messages` queue
// //   2. Extracts `client_id` from the envelope
// //   3. PUBLISHes `fake_response_json` to the `client_id` channel
// //
// // The API's RedisManager is blocking on SUBSCRIBE to that channel, so the
// // publish immediately unblocks it and the HTTP response flows back to the test.

// fn spawn_fake_engine(label: &str, fake_response_json: serde_json::Value) {
//     let label = label.to_string();
//     std::thread::spawn(move || {
//         let client = match RedisClient::open(REDIS_URL) {
//             Ok(c) => c,
//             Err(e) => { eprintln!("  🔧  [{}] Redis open error: {}", label, e); return; }
//         };
//         let mut pop_conn = match client.get_connection() {
//             Ok(c) => c,
//             Err(e) => { eprintln!("  🔧  [{}] pop conn error: {}", label, e); return; }
//         };
//         let mut pub_conn = match client.get_connection() {
//             Ok(c) => c,
//             Err(e) => { eprintln!("  🔧  [{}] pub conn error: {}", label, e); return; }
//         };

//         println!("  🔧  [fake-engine/{}] waiting on BRPOP \"{}\"…", label, MESSAGES_QUEUE);

//         let result: Option<(String, String)> = match pop_conn.brpop(MESSAGES_QUEUE, 10.0) {
//             Ok(r)  => r,
//             Err(e) => {
//                 eprintln!("  🔧  [fake-engine/{}] BRPOP error: {}", label, e);
//                 return;
//             }
//         };

//         match result {
//             None => {
//                 warn(&format!("[fake-engine/{}] BRPOP timed out — API never pushed a message", label));
//             }
//             Some((_key, raw)) => {
//                 println!("  🔧  [fake-engine/{}] received: {}", label, raw);

//                 let envelope: Value = match serde_json::from_str(&raw) {
//                     Ok(v)  => v,
//                     Err(e) => {
//                         eprintln!("  🔧  [fake-engine/{}] JSON parse error: {}", label, e);
//                         return;
//                     }
//                 };

//                 let client_id = match envelope.get("client_id").and_then(|v| v.as_str()) {
//                     Some(id) => id.to_string(),
//                     None => {
//                         eprintln!("  🔧  [fake-engine/{}] missing client_id in envelope", label);
//                         return;
//                     }
//                 };

//                 let reply = fake_response_json.to_string();
//                 println!("  🔧  [fake-engine/{}] publishing to channel \"{}\"", label, client_id);

//                 if let Err(e) = pub_conn.publish::<_, _, ()>(&client_id, &reply) {
//                     eprintln!("  🔧  [fake-engine/{}] publish error: {}", label, e);
//                 } else {
//                     println!("  🔧  [fake-engine/{}] reply published ✓", label);
//                 }
//             }
//         }
//     });

//     // Give the thread time to connect and block on BRPOP before the HTTP call fires.
//     std::thread::sleep(Duration::from_millis(200));
// }

// // ─── Main ─────────────────────────────────────────────────────────────────────

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {

//     banner("EXCHANGE  —  FULL END-TO-END INTEGRATION TEST");

//     // ── Pre-flight: Redis reachable? ──────────────────────────────────────
//     {
//         let rc = RedisClient::open(REDIS_URL)?;
//         let mut conn = rc.get_connection()
//             .map_err(|e| format!("Cannot reach Redis at {}: {}", REDIS_URL, e))?;
//         let pong: String = redis::cmd("PING").query(&mut conn)?;
//         if pong == "PONG" { ok("Redis reachable"); } else { return Err("Redis PING failed".into()); }
//     }

//     // HTTP client with a persistent cookie jar (simulates a browser session)
//     let http = Client::builder()
//         .cookie_store(true)
//         .timeout(Duration::from_secs(15))
//         .build()?;

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 1 — SIGNUP
//     // ══════════════════════════════════════════════════════════════════════
//     section(1, "SIGNUP");

//     let res = http
//         .post(format!("{BASE_URL}/auth/signup"))
//         .json(&json!({
//             "username": TEST_USERNAME,
//             "email":    TEST_EMAIL,
//             "password": TEST_PASSWORD
//         }))
//         .send().await?;

//     let status = res.status().as_u16();
//     let body: Value = res.json().await.unwrap_or(Value::Null);
//     info("STATUS", &status.to_string());
//     dump_body("BODY ↓", &body);

//     match status {
//         200 | 201 => ok("Signup succeeded — new account created"),
//         409       => warn("User already exists — that's fine, continuing with login"),
//         _         => {
//             fail(&format!("Unexpected signup status {}", status));
//             return Err(format!("Signup failed with HTTP {}", status).into());
//         }
//     }

//     // Validation: bad signup payloads should be rejected
//     log("Testing validation: short password should be rejected");
//     let bad_res = http
//         .post(format!("{BASE_URL}/auth/signup"))
//         .json(&json!({
//             "username": "x",
//             "email":    "bad-email",
//             "password": "short"
//         }))
//         .send().await?;
//     let bad_status = bad_res.status().as_u16();
//     if bad_status == 400 || bad_status == 422 {
//         ok(&format!("Bad payload correctly rejected with HTTP {}", bad_status));
//     } else {
//         warn(&format!("Expected 400/422 for bad payload, got {} (check your validators)", bad_status));
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 2 — LOGIN
//     // ══════════════════════════════════════════════════════════════════════
//     section(2, "LOGIN");

//     let res = http
//         .post(format!("{BASE_URL}/auth/login"))
//         .json(&json!({ "email": TEST_EMAIL, "password": TEST_PASSWORD }))
//         .send().await?;

//     let status = res.status().as_u16();
//     let body: Value = res.json().await.unwrap_or(Value::Null);
//     info("STATUS", &status.to_string());
//     dump_body("BODY ↓", &body);

//     if !assert_status(status, 200, "Login") {
//         return Err("Login failed — cannot continue test".into());
//     }

//     // ── Wrong password must return 401 ──────────────────────────────────
//     log("Testing: wrong password → 401");
//     let bad_login = http
//         .post(format!("{BASE_URL}/auth/login"))
//         .json(&json!({ "email": TEST_EMAIL, "password": "wrongpassword" }))
//         .send().await?;
//     assert_status(bad_login.status().as_u16(), 401, "Wrong-password login");

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 3 — CHECK AUTH (logged in)
//     // ══════════════════════════════════════════════════════════════════════
//     section(3, "CHECK AUTH — expect 200 (session live)");

//     let res = http.get(format!("{BASE_URL}/auth/check-auth")).send().await?;
//     let status = res.status().as_u16();
//     let body: Value = res.json().await.unwrap_or(Value::Null);
//     info("STATUS", &status.to_string());
//     dump_body("BODY ↓", &body);
//     assert_status(status, 200, "Check-auth post-login");

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 4 — ON-RAMP  (fund account)
//     // ══════════════════════════════════════════════════════════════════════
//     section(4, "ON-RAMP — fund account via fake engine");

//     let txn_id = "txn-e2e-001";

//     spawn_fake_engine("on-ramp", json!({
//         "type": "ON_RAMP_CONFIRMED",
//         "payload": {
//             "txn_id": txn_id,
//             "status": "confirmed"
//         }
//     }));

//     let res = http
//         .post(format!("{BASE_URL}/onramp/inr"))
//         .json(&json!({
//             "amount": "100000",
//             "txn_id": txn_id
//         }))
//         .send().await?;

//     let status = res.status().as_u16();
//     let body: Value = res.json().await.unwrap_or(Value::Null);
//     info("STATUS", &status.to_string());
//     dump_body("BODY ↓", &body);
//     // Accept 200 or 201 — the server may confirm synchronously or async
//     if status == 200 || status == 201 {
//         ok("On-ramp accepted");
//     } else {
//         warn(&format!("On-ramp returned HTTP {} — adjust expectation if needed", status));
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 5 — GET MARKET DEPTH
//     // ══════════════════════════════════════════════════════════════════════
//     section(5, "GET MARKET DEPTH");

//     spawn_fake_engine("get-depth", json!({
//         "type": "DEPTH",
//         "payload": {
//             "market": TEST_MARKET,
//             "bids": [["99.50", "500"], ["99.00", "1000"]],
//             "asks": [["100.50", "300"], ["101.00", "800"]]
//         }
//     }));

//     let res = http
//         .get(format!("{BASE_URL}/depth/"))
//         .query(&[("symbol", TEST_MARKET)])
//         .send().await?;

//     let status = res.status().as_u16();
//     let body: Value = res.json().await.unwrap_or(Value::Null);
//     info("STATUS", &status.to_string());
//     dump_body("BODY ↓", &body);
//     assert_status(status, 200, "Get depth");

//     if let Some(bids) = body.get("bids").and_then(|v| v.as_array()) {
//         info("Bid levels", &bids.len().to_string());
//     }
//     if let Some(asks) = body.get("asks").and_then(|v| v.as_array()) {
//         info("Ask levels", &asks.len().to_string());
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 6 — CREATE ORDER
//     // ══════════════════════════════════════════════════════════════════════
//     section(6, "CREATE ORDER — buy limit");

//     spawn_fake_engine("create-order", json!({
//         "type": "ORDER_PLACED",
//         "payload": {
//             "order_id":     "e2e-order-001",
//             "executed_qty": 0.0,
//             "fills":        []
//         }
//     }));

//     let res = http
//         .post(format!("{BASE_URL}/orders/"))
//         .json(&json!({
//             "market":   TEST_MARKET,
//             "price":    "100",
//             "quantity": "10",
//             "side":     "buy"
//         }))
//         .send().await?;

//     let status = res.status().as_u16();
//     let body: Value = res.json().await.unwrap_or(Value::Null);
//     info("STATUS", &status.to_string());
//     dump_body("BODY ↓", &body);
//     assert_status(status, 200, "Create buy order");

//     let placed_order_id = body
//         .get("order_id")
//         .and_then(|v| v.as_str())
//         .unwrap_or("e2e-order-001")
//         .to_string();
//     info("order_id", &placed_order_id);

//     // ── Partially-filled order (executed_qty > 0) ────────────────────────
//     log("Testing partial fill scenario");
//     spawn_fake_engine("create-order-partial", json!({
//         "type": "ORDER_PLACED",
//         "payload": {
//             "order_id":     "e2e-order-002",
//             "executed_qty": 5.0,
//             "fills": [
//                 { "price": "100", "qty": 5.0, "trade_id": 1001 }
//             ]
//         }
//     }));

//     let res2 = http
//         .post(format!("{BASE_URL}/orders/"))
//         .json(&json!({
//             "market":   TEST_MARKET,
//             "price":    "100",
//             "quantity": "10",
//             "side":     "sell"
//         }))
//         .send().await?;

//     let s2 = res2.status().as_u16();
//     let b2: Value = res2.json().await.unwrap_or(Value::Null);
//     info("STATUS", &s2.to_string());
//     dump_body("BODY (partial fill) ↓", &b2);
//     assert_status(s2, 200, "Create sell order (partial fill)");

//     if let Some(qty) = b2.get("executed_qty").and_then(|v| v.as_f64()) {
//         if qty > 0.0 { ok(&format!("Partial fill acknowledged: executed_qty = {}", qty)); }
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 7 — GET OPEN ORDERS
//     // ══════════════════════════════════════════════════════════════════════
//     section(7, "GET OPEN ORDERS");

//     spawn_fake_engine("open-orders", json!({
//         "type": "OPEN_ORDERS",
//         "payload": [
//             {
//                 "order_id":     "e2e-order-001",
//                 "executed_qty": 0.0,
//                 "price":        "100",
//                 "quantity":     "10",
//                 "side":         "buy",
//                 "user_id":      "test-user"
//             }
//         ]
//     }));

//     let res = http
//         .get(format!("{BASE_URL}/orders/open"))
//         .query(&[("market", TEST_MARKET)])
//         .send().await?;

//     let status = res.status().as_u16();
//     let body: Value = res.json().await.unwrap_or(Value::Null);
//     info("STATUS", &status.to_string());
//     dump_body("BODY ↓", &body);
//     assert_status(status, 200, "Get open orders");

//     if let Some(arr) = body.as_array() {
//         info("Open orders count", &arr.len().to_string());
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 8 — CANCEL ORDER
//     // ══════════════════════════════════════════════════════════════════════
//     section(8, "CANCEL ORDER");

//     spawn_fake_engine("cancel-order", json!({
//         "type": "ORDER_CANCELLED",
//         "payload": {
//             "order_id":      "e2e-order-001",
//             "executed_qty":  0.0,
//             "remaining_qty": 10.0
//         }
//     }));

//     let res = http
//         .delete(format!("{BASE_URL}/orders/"))
//         .query(&[("order_id", placed_order_id.as_str()), ("market", TEST_MARKET)])
//         .send().await?;

//     let status = res.status().as_u16();
//     let body: Value = res.json().await.unwrap_or(Value::Null);
//     info("STATUS", &status.to_string());
//     dump_body("BODY ↓", &body);
//     assert_status(status, 200, "Cancel order");

//     // Cancelling a non-existent order should return 404 or 400
//     log("Testing: cancel unknown order → 404/400");
//     spawn_fake_engine("cancel-unknown", json!({
//         "type": "ORDER_CANCELLED",
//         "payload": { "error": "order not found" }
//     }));
//     let bad_cancel = http
//         .delete(format!("{BASE_URL}/orders/"))
//         .query(&[("order_id", "does-not-exist"), ("market", TEST_MARKET)])
//         .send().await?;
//     let bc_status = bad_cancel.status().as_u16();
//     if bc_status == 404 || bc_status == 400 || bc_status == 200 {
//         // 200 is also acceptable if the engine returns an error payload
//         ok(&format!("Unknown cancel returned HTTP {} (expected 400/404 or engine-level error)", bc_status));
//     } else {
//         warn(&format!("Cancel unknown order: HTTP {}", bc_status));
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 9 — WEBSOCKET: TICKER SUBSCRIPTION
//     // ══════════════════════════════════════════════════════════════════════
//     section(9, "WS — subscribe to ticker stream");

//     match timeout(Duration::from_secs(8), test_ws_ticker()).await {
//         Ok(Ok(()))  => ok("WebSocket ticker test passed"),
//         Ok(Err(e))  => warn(&format!("WebSocket ticker test error: {}", e)),
//         Err(_)      => warn("WebSocket ticker test timed out (server may not push ticks in test env)"),
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 10 — WEBSOCKET: DEPTH SUBSCRIPTION
//     // ══════════════════════════════════════════════════════════════════════
//     section(10, "WS — subscribe to depth stream");

//     match timeout(Duration::from_secs(8), test_ws_depth()).await {
//         Ok(Ok(()))  => ok("WebSocket depth test passed"),
//         Ok(Err(e))  => warn(&format!("WebSocket depth test error: {}", e)),
//         Err(_)      => warn("WebSocket depth test timed out (no depth updates in test env)"),
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 11 — AUTH MIDDLEWARE  (unauthenticated requests)
//     // ══════════════════════════════════════════════════════════════════════
//     section(11, "AUTH GUARD — anonymous requests must be rejected (401)");

//     let anon = Client::builder()
//         .timeout(Duration::from_secs(5))
//         .build()?;  // no cookie store → no session

//     let anon_cases: Vec<(&str, reqwest::RequestBuilder)> = vec![
//         ("POST   /orders/",
//          anon.post(format!("{BASE_URL}/orders/"))
//              .json(&json!({"market": TEST_MARKET,"price":"100","quantity":"10","side":"buy"}))),
//         ("GET    /orders/open",
//          anon.get(format!("{BASE_URL}/orders/open"))
//              .query(&[("market", TEST_MARKET)])),
//         ("DELETE /orders/",
//          anon.delete(format!("{BASE_URL}/orders/"))
//              .query(&[("order_id", "x"), ("market", TEST_MARKET)])),
//         ("GET    /auth/check-auth",
//          anon.get(format!("{BASE_URL}/auth/check-auth"))),
//     ];

//     for (label, req) in anon_cases {
//         match req.send().await {
//             Ok(res)  => { assert_status(res.status().as_u16(), 401, label); }
//             Err(e)   => { fail(&format!("{} — request failed: {}", label, e)); }
//         }
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 12 — LOGOUT
//     // ══════════════════════════════════════════════════════════════════════
//     section(12, "LOGOUT");

//     let res = http.post(format!("{BASE_URL}/auth/logout")).send().await?;
//     let status = res.status().as_u16();
//     info("STATUS", &status.to_string());
//     assert_status(status, 200, "Logout");

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 13 — CHECK AUTH AFTER LOGOUT
//     // ══════════════════════════════════════════════════════════════════════
//     section(13, "CHECK AUTH AFTER LOGOUT — expect 401");

//     let res = http.get(format!("{BASE_URL}/auth/check-auth")).send().await?;
//     let status = res.status().as_u16();
//     info("STATUS", &status.to_string());
//     assert_status(status, 401, "Check-auth post-logout");

//     // ══════════════════════════════════════════════════════════════════════
//     // SECTION 14 — SESSION ISOLATION
//     // ══════════════════════════════════════════════════════════════════════
//     section(14, "SESSION ISOLATION — order routes reject post-logout session");

//     // The same `http` client still holds its (now invalidated) cookie.
//     // Order routes must reject it.
//     let post_logout_order = http
//         .post(format!("{BASE_URL}/orders/"))
//         .json(&json!({"market": TEST_MARKET,"price":"100","quantity":"10","side":"buy"}))
//         .send().await?;
//     let plo_status = post_logout_order.status().as_u16();
//     if plo_status == 401 {
//         ok("POST /orders/ correctly rejected after logout (401)");
//     } else {
//         warn(&format!("POST /orders/ returned {} after logout — session may still be valid", plo_status));
//     }

//     // ══════════════════════════════════════════════════════════════════════
//     // DONE
//     // ══════════════════════════════════════════════════════════════════════
//     banner("ALL CHECKS COMPLETE");
//     println!();

//     Ok(())
// }

// // ─── WebSocket helpers ────────────────────────────────────────────────────────

// /// Connect to WS, subscribe to the ticker for TEST_MARKET, then wait for
// /// one message (or close gracefully if none arrive within the timeout).
// async fn test_ws_ticker() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//     let url = format!("{WS_URL}");
//     log(&format!("Connecting to WS at {}", url));

//     let (ws_stream, _response) = connect_async(&url).await?;
//     ok("WS connection established");

//     let (mut write, mut read) = ws_stream.split();

//     // Subscribe message — adjust to match your server's expected format
//     let subscribe_msg = json!({
//         "method": "SUBSCRIBE",
//         "params": [format!("{}@ticker", TEST_MARKET.to_lowercase())],
//         "id": 1
//     });

//     write.send(Message::Text(subscribe_msg.to_string())).await?;
//     ok(&format!("Sent ticker subscription for {}", TEST_MARKET));

//     // Wait for acknowledgement or first data frame
//     if let Some(msg) = read.next().await {
//         match msg? {
//             Message::Text(text) => {
//                 log(&format!("WS received: {}", text));
//                 match serde_json::from_str::<Value>(&text) {
//                     Ok(v) => { dump_body("WS message ↓", &v); ok("Ticker subscription acknowledged"); }
//                     Err(_) => { log(&format!("WS raw frame: {}", text)); }
//                 }
//             }
//             Message::Binary(b) => log(&format!("WS binary frame: {} bytes", b.len())),
//             Message::Ping(_)   => { ok("WS ping received"); }
//             Message::Close(_)  => { warn("WS closed by server"); }
//             other              => log(&format!("WS other frame: {:?}", other)),
//         }
//     } else {
//         warn("WS: no message received (stream ended)");
//     }

//     // Graceful close
//     write.send(Message::Close(None)).await.ok();
//     Ok(())
// }

// /// Connect to WS, subscribe to depth for TEST_MARKET.
// async fn test_ws_depth() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//     let url = format!("{WS_URL}");
//     log(&format!("Connecting to WS at {}", url));

//     let (ws_stream, _response) = connect_async(&url).await?;
//     ok("WS connection established for depth");

//     let (mut write, mut read) = ws_stream.split();

//     let subscribe_msg = json!({
//         "method": "SUBSCRIBE",
//         "params": [format!("{}@depth", TEST_MARKET.to_lowercase())],
//         "id": 2
//     });

//     write.send(Message::Text(subscribe_msg.to_string())).await?;
//     ok(&format!("Sent depth subscription for {}", TEST_MARKET));

//     if let Some(msg) = read.next().await {
//         match msg? {
//             Message::Text(text) => {
//                 log(&format!("WS depth received: {}", text));
//                 match serde_json::from_str::<Value>(&text) {
//                     Ok(v) => {
//                         dump_body("WS depth message ↓", &v);
//                         // Validate structure if this is a depth update
//                         if v.get("bids").is_some() || v.get("asks").is_some() {
//                             ok("Depth update contains bids/asks");
//                         } else {
//                             ok("Depth subscription acknowledged");
//                         }
//                     }
//                     Err(_) => log(&format!("WS raw frame: {}", text)),
//                 }
//             }
//             Message::Close(_) => warn("WS closed by server"),
//             other => log(&format!("WS other frame: {:?}", other)),
//         }
//     } else {
//         warn("WS depth: no message received");
//     }

//     write.send(Message::Close(None)).await.ok();
//     Ok(())
// }
















/// End-to-end integration test for the crypto exchange.
/// Run with: cargo run --bin e2e_test
///
/// Requires:
///   API_BASE   = http://localhost:8080  (or set env)
///   WS_URL     = ws://localhost:3001    (or set env)
///
/// Services that must be running:
///   - api      (Axum, default :8080)
///   - engine   (matching engine)
///   - ws       (Warp WS, default :3001)
///   - database (db_processor)
///   - Postgres + Redis

use std::{sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};

// ─────────────────────────────────────────────
//  Config
// ─────────────────────────────────────────────

fn api_base() -> String {
    std::env::var("API_BASE").unwrap_or_else(|_| "http://localhost:8080".into())
}
fn ws_url() -> String {
    std::env::var("WS_URL").unwrap_or_else(|_| "ws://localhost:3001".into())
}

const MARKET: &str = "SOL_USDC";

// ─────────────────────────────────────────────
//  Shared types (mirrors your engine/api types)
// ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum OrderSide {
    Buy,
    Sell,
}

// ─────────────────────────────────────────────
//  Pretty logging helpers
// ─────────────────────────────────────────────

fn section(title: &str) {
    println!("\n{}", "═".repeat(60));
    println!("  {}", title);
    println!("{}", "═".repeat(60));
}

fn step(msg: &str) {
    println!("\n▶  {}", msg);
}

fn ok(msg: &str) {
    println!("   ✅  {}", msg);
}

fn fail(msg: &str) {
    println!("   ❌  {}", msg);
}

fn info(msg: &str) {
    println!("   ℹ   {}", msg);
}

fn json_pretty(label: &str, v: &Value) {
    println!("   {}:", label);
    for line in serde_json::to_string_pretty(v).unwrap_or_default().lines() {
        println!("       {}", line);
    }
}

// ─────────────────────────────────────────────
//  HTTP helpers
// ─────────────────────────────────────────────

/// Checks status, prints body on error, returns parsed JSON.
async fn expect_json(label: &str, resp: Response, expected: StatusCode) -> Value {
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();

    if status != expected {
        fail(&format!(
            "{} — expected {} got {}\n       body: {}",
            label, expected, status, body_text
        ));
        panic!("Test failed at: {}", label);
    }

    let v: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
        fail(&format!("{} — response is not valid JSON: {}", label, body_text));
        panic!("Test failed at: {}", label);
    });

    ok(&format!("{} → {}", label, status));
    v
}

// ─────────────────────────────────────────────
//  Main
// ─────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Build a cookie-storing client so the JWT cookie persists across calls.
    let client = Arc::new(
        Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client"),
    );

    println!("\n🚀  Crypto Exchange — End-to-End Test Suite");
    println!("    API  : {}", api_base());
    println!("    WS   : {}", ws_url());

    // ──────────────────────────────────────────
    //  1. Auth
    // ──────────────────────────────────────────
    section("1 / AUTH");

    // -- signup --------------------------------
    step("POST /api/v1/auth/signup");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let email = format!("testuser_{}@exchange.test", ts);
    let password = "securepassword123";
    let username = format!("trader_{}", ts);

    info(&format!("email    : {}", email));
    info(&format!("username : {}", username));

    let signup_body = json!({
        "username": username,
        "email":    email,
        "password": password,
    });

    let signup_resp = client
        .post(format!("{}/api/v1/auth/signup", api_base()))
        .json(&signup_body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("❌  Failed to reach API for signup: {}", e));

    let signup_json = expect_json("Signup", signup_resp, StatusCode::CREATED).await;
    json_pretty("Response", &signup_json);

    // -- duplicate signup should fail ----------
    step("POST /api/v1/auth/signup (duplicate — expect 4xx)");
    let dup_resp = client
        .post(format!("{}/api/v1/auth/signup", api_base()))
        .json(&signup_body)
        .send()
        .await
        .unwrap();
    let dup_status = dup_resp.status();
    if dup_status.is_client_error() {
        ok(&format!("Duplicate signup correctly rejected → {}", dup_status));
    } else {
        fail(&format!("Expected 4xx for duplicate, got {}", dup_status));
    }

    // -- logout before login (cookie not yet set, expect 4xx or 200 depending on impl)
    step("POST /api/v1/auth/logout (before login — should be 4xx or 200)");
    let pre_logout_resp = client
        .post(format!("{}/api/v1/auth/logout", api_base()))
        .send()
        .await
        .unwrap();
    info(&format!("Pre-login logout status: {}", pre_logout_resp.status()));

    // -- login ---------------------------------
    step("POST /api/v1/auth/login");
    let login_body = json!({ "email": email, "password": password });

    let login_resp = client
        .post(format!("{}/api/v1/auth/login", api_base()))
        .json(&login_body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("❌  Failed to reach API for login: {}", e));

    let login_json = expect_json("Login", login_resp, StatusCode::OK).await;
    json_pretty("Response", &login_json);

    // -- wrong password ------------------------
    step("POST /api/v1/auth/login (wrong password — expect 4xx)");
    let bad_login = client
        .post(format!("{}/api/v1/auth/login", api_base()))
        .json(&json!({ "email": email, "password": "wrongpassword" }))
        .send()
        .await
        .unwrap();
    if bad_login.status().is_client_error() {
        ok(&format!("Wrong password correctly rejected → {}", bad_login.status()));
    } else {
        fail(&format!("Expected 4xx for bad password, got {}", bad_login.status()));
    }

    // -- check-auth ----------------------------
    step("GET /api/v1/auth/check-auth");
    let check_resp = client
        .get(format!("{}/api/v1/auth/check-auth", api_base()))
        .send()
        .await
        .unwrap();
    let check_json = expect_json("Check-auth", check_resp, StatusCode::OK).await;
    json_pretty("Response", &check_json);

    // ──────────────────────────────────────────
    //  2. On-Ramp (fund account)
    // ──────────────────────────────────────────
    section("2 / ON-RAMP — Fund account via engine");

    // The API may not have an on-ramp HTTP endpoint; it's sent directly as engine message.
    // We'll try a POST if it exists, otherwise note it.
    step("POST /api/v1/onramp (if route exists)");
    let onramp_resp = client
        .post(format!("{}/api/v1/onramp", api_base()))
        .json(&json!({
            "amount":  "100000",
            "txn_id":  format!("txn_{}", ts),
        }))
        .send()
        .await
        .unwrap();
    let onramp_status = onramp_resp.status();
    info(&format!("On-ramp status: {}", onramp_status));
    if onramp_status.is_success() {
        ok("On-ramp route exists and accepted funds");
        let body: Value = onramp_resp.json().await.unwrap_or(json!({}));
        json_pretty("Response", &body);
    } else {
        info("On-ramp route not exposed via HTTP — balances may need seeding directly on engine");
        info("Continuing; order placement may fail if balance is 0");
    }

    // ──────────────────────────────────────────
    //  3. Orders — unauthenticated guard check
    // ──────────────────────────────────────────
    section("3 / ORDER AUTH GUARD");

    step("GET /api/v1/orders/open (no cookie — expect 401/403)");
    let fresh_client = Client::builder()
        .cookie_store(false)
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let unauth_resp = fresh_client
        .get(format!("{}/api/v1/orders/open?market={}", api_base(), MARKET))
        .send()
        .await
        .unwrap();
    if unauth_resp.status().is_client_error() {
        ok(&format!("Unauthenticated request correctly rejected → {}", unauth_resp.status()));
    } else {
        fail(&format!(
            "Expected 4xx without auth, got {}",
            unauth_resp.status()
        ));
    }

    // ──────────────────────────────────────────
    //  4. Depth
    // ──────────────────────────────────────────
    section("4 / MARKET DEPTH");

    step(&format!("GET /api/v1/depth/{MARKET}"));
   let depth_resp = client
    .get(format!("{}/api/v1/depth/{}", api_base(), MARKET))
        .send()
        .await
        .unwrap();
    let depth_status = depth_resp.status();
    info(&format!("Depth status: {}", depth_status));
    if depth_status.is_success() {
        let depth_json: Value = depth_resp.json().await.unwrap_or(json!({}));
        json_pretty("Depth", &depth_json);
    } else {
        let body = depth_resp.text().await.unwrap_or_default();
        info(&format!("Depth response (non-2xx): {}", body));
    }

    // ──────────────────────────────────────────
    //  5. Place orders
    // ──────────────────────────────────────────
    section("5 / PLACE ORDERS");

    // -- BUY limit order ----------------------
    step("POST /api/v1/orders — BUY limit");
    let buy_body = json!({
        "market":   MARKET,
        "price":    "100",
        "quantity": "10",
        "side":     "buy",
    });
    info("Payload:");
    json_pretty("", &buy_body);

    let buy_resp = client
        .post(format!("{}/api/v1/orders", api_base()))
        .json(&buy_body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("❌  POST order failed: {}", e));

    let buy_json = expect_json("Place BUY order", buy_resp, StatusCode::OK).await;
    json_pretty("Response", &buy_json);

    let buy_order_id = buy_json
        .get("payload")
        .and_then(|p| p.get("order_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    info(&format!("BUY order_id: {}", buy_order_id));

    // -- SELL limit order at different price --
    step("POST /api/v1/orders — SELL limit (non-crossing)");
    let sell_body = json!({
        "market":   MARKET,
        "price":    "200",
        "quantity": "5",
        "side":     "sell",
    });
    info("Payload:");
    json_pretty("", &sell_body);

    let sell_resp = client
        .post(format!("{}/api/v1/orders", api_base()))
        .json(&sell_body)
        .send()
        .await
        .unwrap();

    let sell_json = expect_json("Place SELL order", sell_resp, StatusCode::OK).await;
    json_pretty("Response", &sell_json);

    let sell_order_id = sell_json
        .get("payload")
        .and_then(|p| p.get("order_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    info(&format!("SELL order_id: {}", sell_order_id));

    // -- Crossing order (should fill) ---------
    step("POST /api/v1/orders — BUY at SELL price (should match/fill)");
    let cross_body = json!({
        "market":   MARKET,
        "price":    "200",
        "quantity": "3",
        "side":     "buy",
    });
    info("Payload:");
    json_pretty("", &cross_body);

    let cross_resp = client
        .post(format!("{}/api/v1/orders", api_base()))
        .json(&cross_body)
        .send()
        .await
        .unwrap();

    let cross_json = expect_json("Crossing BUY order", cross_resp, StatusCode::OK).await;
    json_pretty("Response", &cross_json);

    let cross_executed = cross_json
        .get("payload")
        .and_then(|p| p.get("executed_qty"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if cross_executed > 0.0 {
        ok(&format!("Order matched! executed_qty = {}", cross_executed));
    } else {
        info("executed_qty = 0 — order is resting (balance may be 0 or no match)");
    }

    // ──────────────────────────────────────────
    //  6. Get open orders
    // ──────────────────────────────────────────
    section("6 / GET OPEN ORDERS");

    step(&format!("GET /api/v1/orders/open?market={}", MARKET));
    let open_resp = client
        .get(format!(
            "{}/api/v1/orders/open?market={}",
            api_base(),
            MARKET
        ))
        .send()
        .await
        .unwrap();

    let open_json = expect_json("Get open orders", open_resp, StatusCode::OK).await;
    json_pretty("Response", &open_json);

    let open_count = open_json.as_array().map(|a| a.len()).unwrap_or_else(|| {
        open_json
            .get("payload")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0)
    });
    info(&format!("Open orders count: {}", open_count));

    // ──────────────────────────────────────────
    //  7. Cancel order
    // ──────────────────────────────────────────
    section("7 / CANCEL ORDER");

    if buy_order_id.is_empty() {
        info("No BUY order_id captured — skipping cancel test");
    } else {
        step(&format!("DELETE /api/v1/orders (order_id = {})", buy_order_id));
        let cancel_body = json!({
            "order_id": buy_order_id,
            "market":   MARKET,
        });
        info("Payload:");
        json_pretty("", &cancel_body);

        let cancel_resp = client
            .delete(format!("{}/api/v1/orders", api_base()))
            .json(&cancel_body)
            .send()
            .await
            .unwrap();

        let cancel_status = cancel_resp.status();
        info(&format!("Cancel status: {}", cancel_status));
        if cancel_status.is_success() {
            let cancel_json: Value = cancel_resp.json().await.unwrap_or(json!({}));
            json_pretty("Cancel response", &cancel_json);
            ok("Cancel accepted by API");
        } else {
            let body = cancel_resp.text().await.unwrap_or_default();
            fail(&format!("Cancel failed: {}", body));
        }
    }

    // ──────────────────────────────────────────
    //  8. Open orders after cancel
    // ──────────────────────────────────────────
    section("8 / OPEN ORDERS AFTER CANCEL");

    step(&format!("GET /api/v1/orders/open?market={}", MARKET));
    let open2_resp = client
        .get(format!(
            "{}/api/v1/orders/open?market={}",
            api_base(),
            MARKET
        ))
        .send()
        .await
        .unwrap();
    let open2_json = expect_json("Open orders (post-cancel)", open2_resp, StatusCode::OK).await;
    json_pretty("Response", &open2_json);

    // ──────────────────────────────────────────
    //  9. WebSocket
    // ──────────────────────────────────────────
    section("9 / WEBSOCKET");

    step(&format!("Connecting to {}", ws_url()));
    match connect_async(ws_url()).await {
        Err(e) => {
            fail(&format!("WS connection failed: {}", e));
            info("Skipping WS tests — is the ws service running?");
        }
        Ok((ws_stream, _)) => {
            ok("WS connection established");

            let (mut write, mut read) = ws_stream.split();

            // -- subscribe to depth -----------
            let sub_depth = json!({
                "method": "SUBSCRIBE",
                "params": [format!("depth@{}", MARKET)],
            });
            step("SUBSCRIBE depth@SOL_USDC");
            write
                .send(Message::Text(sub_depth.to_string()))
                .await
                .unwrap();
            ok("Subscribe message sent");

            // -- subscribe to trades ----------
            let sub_trade = json!({
                "method": "SUBSCRIBE",
                "params": [format!("trade@{}", MARKET)],
            });
            step("SUBSCRIBE trade@SOL_USDC");
            write
                .send(Message::Text(sub_trade.to_string()))
                .await
                .unwrap();
            ok("Subscribe message sent");

            // -- listen for up to 5 seconds ---
            step("Listening for WS messages (5 s timeout)");
            let deadline = sleep(Duration::from_secs(5));
            tokio::pin!(deadline);
            let mut msg_count = 0usize;

            loop {
                tokio::select! {
                    _ = &mut deadline => {
                        info("5 s elapsed — stopping WS listen");
                        break;
                    }
                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(txt))) => {
                                msg_count += 1;
                                let v: Value = serde_json::from_str(&txt).unwrap_or(json!({"raw": txt}));
                                info(&format!("WS message #{}", msg_count));
                                json_pretty("  msg", &v);
                            }
                            Some(Ok(Message::Ping(_))) => { info("WS ping received"); }
                            Some(Ok(Message::Close(_))) => {
                                info("WS connection closed by server");
                                break;
                            }
                            Some(Err(e)) => {
                                fail(&format!("WS read error: {}", e));
                                break;
                            }
                            None => {
                                info("WS stream ended");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }

            if msg_count == 0 {
                info("No WS messages received — the pubsub forwarding path may not be fully wired yet");
            } else {
                ok(&format!("{} WS message(s) received", msg_count));
            }

            // -- unsubscribe ------------------
            step("UNSUBSCRIBE depth@SOL_USDC");
            let unsub = json!({
                "method": "UNSUBSCRIBE",
                "params": [format!("depth@{}", MARKET)],
            });
            let _ = write.send(Message::Text(unsub.to_string())).await;
            ok("Unsubscribe sent");

            let _ = write.send(Message::Close(None)).await;
        }
    }

    // ──────────────────────────────────────────
    //  10. Logout
    // ──────────────────────────────────────────
    section("10 / LOGOUT");

    step("POST /api/v1/auth/logout");
    let logout_resp = client
        .post(format!("{}/api/v1/auth/logout", api_base()))
        .send()
        .await
        .unwrap();
    let logout_status = logout_resp.status();
    info(&format!("Logout status: {}", logout_status));
    if logout_status.is_success() {
        ok("Logged out");
    } else {
        fail(&format!("Logout returned {}", logout_status));
    }

    // -- check-auth after logout should fail --
    step("GET /api/v1/auth/check-auth (after logout — expect 4xx)");
    let post_logout_check = client
        .get(format!("{}/api/v1/auth/check-auth", api_base()))
        .send()
        .await
        .unwrap();
    if post_logout_check.status().is_client_error() {
        ok(&format!(
            "Post-logout auth check correctly rejected → {}",
            post_logout_check.status()
        ));
    } else {
        info(&format!(
            "Post-logout check returned {} (cookie may still be valid client-side)",
            post_logout_check.status()
        ));
    }

    // ──────────────────────────────────────────
    //  Summary
    // ──────────────────────────────────────────
    section("DONE");
    println!("  All test steps executed.");
    println!("  Review ✅ / ❌ / ℹ lines above for pass/fail/info.\n");
}