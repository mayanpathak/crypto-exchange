use redis::{Client as RedisClient, Commands, PubSubCommands};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::sleep;

const BASE_URL: &str = "http://localhost:8080/api/v1";
const REDIS_URL: &str = "redis://localhost:6380";
const MESSAGES_QUEUE: &str = "messages";

// ─── Display helpers ────────────────────────────────────────────────────────

fn section(n: u8, title: &str) {
    println!();
    println!("┌─────────────────────────────────────────────┐");
    println!("│  {}. {}  ", n, title);
    println!("└─────────────────────────────────────────────┘");
}

fn ok(msg: &str)   { println!("  ✅  {}", msg); }
fn fail(msg: &str) { println!("  ❌  {}", msg); }
fn info(label: &str, value: &str) { println!("  {:<14}: {}", label, value); }

fn assert_status(got: u16, expected: u16, label: &str) -> bool {
    if got == expected {
        ok(&format!("{} — got expected {}", label, expected));
        true
    } else {
        fail(&format!("{} — expected {} got {}", label, expected, got));
        false
    }
}

// ─── Fake engine ────────────────────────────────────────────────────────────
//
// Spawned as a background task BEFORE the HTTP calls that need engine replies.
// It:
//   1. BRPOPs one message off the `messages` queue
//   2. Parses the `client_id` from the message
//   3. Builds a plausible fake response matching the message type
//   4. PUBLISHes it to the `client_id` channel
//
// The API's RedisManager is blocking on SUBSCRIBE to that same channel,
// so the moment we PUBLISH it unblocks and the HTTP response flows back.

fn spawn_fake_engine(fake_response_json: serde_json::Value) {
    std::thread::spawn(move || {
        let client = RedisClient::open(REDIS_URL)
            .expect("engine: redis open");

        // Two separate connections: one for BRPOP, one for PUBLISH
        // (redis-rs doesn't allow commands on a PubSub connection)
        let mut pop_conn = client.get_connection().expect("engine: pop conn");
        let mut pub_conn = client.get_connection().expect("engine: pub conn");

        println!("  🔧  [fake-engine] waiting on BRPOP \"{}\"...", MESSAGES_QUEUE);

      let result: Option<(String, String)> = match pop_conn.brpop(MESSAGES_QUEUE, 8.0) {
    Ok(r) => r,
    Err(e) => {
        println!("  ⚠️   [fake-engine] BRPOP connection dropped: {}", e);
        return;
    }
};

        match result {
            None => {
                println!("  ⚠️   [fake-engine] BRPOP timed out — API never pushed a message");
            }
            Some((_key, raw)) => {
                println!("  🔧  [fake-engine] received: {}", raw);

                // Extract client_id from the envelope
                let envelope: Value = serde_json::from_str(&raw)
                    .expect("engine: parse envelope");

                let client_id = envelope
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .expect("engine: no client_id in message");

                let reply = fake_response_json.to_string();

                println!(
                    "  🔧  [fake-engine] publishing to channel \"{}\"",
                    client_id
                );

                let _: () = pub_conn
                    .publish(client_id, &reply)
                    .expect("engine: publish");

                println!("  🔧  [fake-engine] reply published ✓");
            }
        }
    });

    // Give the thread a moment to connect and reach BRPOP
    // before the HTTP call is fired, so we never miss the message.
    std::thread::sleep(Duration::from_millis(150));
}

// ─── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("╔══════════════════════════════════════════════╗");
    println!("║      ORDER API  —  INTEGRATION TEST          ║");
    println!("║  (fake engine plays the orderbook role)      ║");
    println!("╚══════════════════════════════════════════════╝");

    // ── Verify Redis is reachable before we start ──────────────────────────
    {
        let rc = RedisClient::open(REDIS_URL)?;
        let mut conn = rc.get_connection()
            .map_err(|e| format!("Cannot reach Redis at {}: {}", REDIS_URL, e))?;
        let pong: String = redis::cmd("PING").query(&mut conn)?;
        if pong != "PONG" { return Err("Redis PING failed".into()); }
        ok("Redis reachable");
    }

    let http = Client::builder()
        .cookie_store(true)
        .build()?;

    let email    = "ordertest@example.com";
    let username = "ordertest";
    let password = "password123";

    // ══════════════════════════════════════════════════════════════════════
    // 1. SIGNUP
    // ══════════════════════════════════════════════════════════════════════
    section(1, "SIGNUP");

    let res = http
        .post(format!("{BASE_URL}/auth/signup"))
        .json(&json!({ "username": username, "email": email, "password": password }))
        .send().await?;

    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    info("STATUS", &status.to_string());
    info("BODY",   &body.to_string());

    match status {
        200 | 201 => ok("Signup succeeded"),
        409       => ok("User already exists — continuing"),
        _         => {
            fail(&format!("Unexpected signup status {}", status));
            return Err(format!("Signup failed: {}", status).into());
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // 2. LOGIN
    // ══════════════════════════════════════════════════════════════════════
    section(2, "LOGIN");

    let res = http
        .post(format!("{BASE_URL}/auth/login"))
        .json(&json!({ "email": email, "password": password }))
        .send().await?;

    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    info("STATUS", &status.to_string());
    info("BODY",   &body.to_string());
    assert_status(status, 200, "Login");

    // ══════════════════════════════════════════════════════════════════════
    // 3. CHECK AUTH  (must pass — cookie is live)
    // ══════════════════════════════════════════════════════════════════════
    section(3, "CHECK AUTH — expect 200");

    let res = http.get(format!("{BASE_URL}/auth/check-auth")).send().await?;
    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    info("STATUS", &status.to_string());
    info("BODY",   &body.to_string());
    assert_status(status, 200, "Check-auth post-login");

    // ══════════════════════════════════════════════════════════════════════
    // 4. CREATE ORDER
    //    Fake engine publishes a plausible ORDER_PLACED response.
    // ══════════════════════════════════════════════════════════════════════
    section(4, "CREATE ORDER");

    spawn_fake_engine(json!({
        "type": "ORDER_PLACED",
        "payload": {
            "order_id":     "fake-order-001",
            "executed_qty": 0.0,
            "fills":        []
        }
    }));

    let res = http
        .post(format!("{BASE_URL}/orders/"))
        .json(&json!({
            "market":   "TATA_INR",
            "price":    "100",
            "quantity": "10",
            "side":     "buy"
        }))
        .send().await?;

    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    info("STATUS", &status.to_string());
    info("BODY",   &serde_json::to_string_pretty(&body).unwrap_or_default());
    assert_status(status, 200, "Create order");

    // ══════════════════════════════════════════════════════════════════════
    // 5. GET OPEN ORDERS
    //    Fake engine publishes a plausible OPEN_ORDERS response.
    // ══════════════════════════════════════════════════════════════════════
    section(5, "GET OPEN ORDERS");

    spawn_fake_engine(json!({
        "type": "OPEN_ORDERS",
        "payload": [
            {
                "order_id":     "fake-order-001",
                "executed_qty": 0.0,
                "price":        "100",
                "quantity":     "10",
                "side":         "buy",
                "user_id":      "test-user"
            }
        ]
    }));

    let res = http
        .get(format!("{BASE_URL}/orders/open"))
        .query(&[("market", "TATA_INR")])
        .send().await?;

    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    info("STATUS", &status.to_string());
    info("BODY",   &serde_json::to_string_pretty(&body).unwrap_or_default());
    assert_status(status, 200, "Get open orders");

    // ══════════════════════════════════════════════════════════════════════
    // 6. CANCEL ORDER
    //    Fake engine publishes a plausible ORDER_CANCELLED response.
    // ══════════════════════════════════════════════════════════════════════
    section(6, "CANCEL ORDER");

    spawn_fake_engine(json!({
        "type": "ORDER_CANCELLED",
        "payload": {
            "order_id":       "fake-order-001",
            "executed_qty":   0.0,
            "remaining_qty":  10.0
        }
    }));

    let res = http
        .delete(format!("{BASE_URL}/orders/"))
        .query(&[("order_id", "fake-order-001"), ("market", "TATA_INR")])
        .send().await?;

    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    info("STATUS", &status.to_string());
    info("BODY",   &serde_json::to_string_pretty(&body).unwrap_or_default());
    assert_status(status, 200, "Cancel order");

    // ══════════════════════════════════════════════════════════════════════
    // 7. ORDER ROUTES REJECT UNAUTHENTICATED REQUESTS
    //    A fresh client (no cookies) must get 401 on all three order routes.
    // ══════════════════════════════════════════════════════════════════════
    section(7, "AUTH MIDDLEWARE — unauthenticated requests rejected");

    let anon = Client::new(); // no cookie store

    let cases = vec![
        ("POST /orders/",      anon.post(format!("{BASE_URL}/orders/"))
                                   .json(&json!({"market":"TATA_INR","price":"100","quantity":"10","side":"buy"}))
                                   .send().await?),
        ("GET /orders/open",   anon.get(format!("{BASE_URL}/orders/open"))
                                   .query(&[("market","TATA_INR")])
                                   .send().await?),
        ("DELETE /orders/",    anon.delete(format!("{BASE_URL}/orders/"))
                                   .query(&[("order_id","x"),("market","TATA_INR")])
                                   .send().await?),
    ];

    for (label, res) in cases {
        let s = res.status().as_u16();
        assert_status(s, 401, label);
    }

    // ══════════════════════════════════════════════════════════════════════
    // 8. LOGOUT
    // ══════════════════════════════════════════════════════════════════════
    section(8, "LOGOUT");

    let res = http.post(format!("{BASE_URL}/auth/logout")).send().await?;
    let status = res.status().as_u16();
    info("STATUS", &status.to_string());
    assert_status(status, 200, "Logout");

    // ══════════════════════════════════════════════════════════════════════
    // 9. CHECK AUTH AFTER LOGOUT  (must fail)
    // ══════════════════════════════════════════════════════════════════════
    section(9, "CHECK AUTH AFTER LOGOUT — expect 401");

    let res = http.get(format!("{BASE_URL}/auth/check-auth")).send().await?;
    let status = res.status().as_u16();
    info("STATUS", &status.to_string());
    assert_status(status, 401, "Check-auth post-logout");

    // ══════════════════════════════════════════════════════════════════════
    // DONE
    // ══════════════════════════════════════════════════════════════════════
    println!();
    println!("╔══════════════════════════════════════════════╗");
    println!("║           ALL CHECKS COMPLETE                ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();

    Ok(())
}