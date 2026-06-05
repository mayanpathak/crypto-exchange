use axum::{
    http::{
        header,
        HeaderValue,
        Method,
    },
    Router,
};
use ::redis::{Client, Commands};



use tower_http::cors::CorsLayer;

use dotenvy::dotenv;


// use axum::middleware;

use std::{
    env,
    net::SocketAddr,
};

use tokio::net::TcpListener;
mod services;
mod routes;
mod types;
mod redis;
mod middleware;
mod controller;
mod utils;

use middleware::auth_middleware;

use database::{
    establish_connection_pool,
    DbPool,
};

use crate::redis::RedisManager;

use routes::{
    auth_routes,
    order_routes,
    depth_routes,
};

#[derive(Clone)]
pub struct AppState {
    pub db_pool: DbPool,
    pub jwt_secret: String,
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    env_logger::init_from_env(
        env_logger::Env::new()
            .default_filter_or("info"),
    );

    // =========================
    // Environment Variables
    // =========================

    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string());

    let addr: SocketAddr = format!("0.0.0.0:{port}")
        .parse()
        .expect("Invalid address");
    let jwt_secret =
    env::var("JWT_SECRET")
        .expect("JWT_SECRET missing");

    // =========================
    // Database
    // =========================

    let db_pool = establish_connection_pool();

    println!(" PostgreSQL pool initialized");

    let state = AppState {
        db_pool,
        jwt_secret,
    };

    // =========================
    // CORS
    // =========================


let frontend_url = env::var("FRONTEND_URL")
    .unwrap_or_else(|_| "http://localhost:3000".to_string());


    let cors = CorsLayer::new()
        .allow_origin(
    frontend_url
        .parse::<HeaderValue>()
        .unwrap(),
)
        .allow_methods([
            Method::GET,
            Method::POST,
                Method::DELETE,

        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
        ])
        .allow_credentials(true);















// =========================
// Redis
// =========================

println!("\n====================================");
println!("Initializing Redis Connection");
println!("====================================");

let redis_url = env::var("REDIS_URL")
    .unwrap_or_else(|_| "redis://localhost:6380".to_string());

println!("Redis URL: {}", redis_url);

let redis_client = Client::open(redis_url.clone())
    .expect("❌ Failed to create Redis client");

println!("✅ Redis client created");

let mut redis_conn = redis_client
    .get_connection()
    .expect("❌ Failed to connect to Redis");

println!("✅ Redis TCP connection established");

let pong: String = ::redis::cmd("PING")
    .query(&mut redis_conn)
    .expect("❌ Redis PING failed");

println!("📡 Redis PING -> {}", pong);

if pong == "PONG" {
    println!("✅ Redis health check passed");
} else {
    panic!("❌ Redis returned unexpected response");
}

// Force initialization of RedisManager singleton
let _redis_manager = RedisManager::get_instance();

println!("✅ RedisManager initialized");
println!("====================================\n");













let api_routes = Router::new()
    .nest("/auth", auth_routes())
    .nest("/orders", order_routes())   // remove trailing slash
    .nest("/depth", depth_routes());


    // =========================
    // App Router
    // =========================

    let app = Router::new()
        .nest("/api/v1", api_routes)
        .with_state(state)
        .layer(cors);

    println!("🚀 Server running on http://{}", addr);

    // =========================
    // TCP Listener
    // =========================

    let listener = TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}