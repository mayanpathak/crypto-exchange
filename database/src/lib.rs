pub mod models;
pub mod schema;

use chrono::{TimeZone, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager};
use log::{error, info, warn};
use redis::Client;
use serde::{Deserialize, Serialize};
use std::env;
use tokio;
use validator::Validate;

use crate::models::{Order, Trade};
use schema::{orders, trades};

pub type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DbMessage {
    TradeAdded(TradeMessage),
    OrderUpdate(OrderMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct TradeMessage {
    #[validate(length(min = 1))]
    pub id: String,
    pub is_buyer_maker: bool,
    pub price: String,
    pub quantity: String,
    pub quote_quantity: String,
    pub timestamp: i64,
    pub market: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct OrderMessage {
    #[validate(length(min = 1))]
    pub order_id: String,
    pub executed_qty: f64,
    pub market: Option<String>,
    pub price: Option<String>,
    pub quantity: Option<String>,
    pub side: Option<String>,
}

pub fn establish_connection_pool() -> DbPool {
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL environment variable is not set");

    info!("Creating PostgreSQL connection pool");

    let manager = ConnectionManager::<PgConnection>::new(database_url);

    let pool = r2d2::Pool::builder()
        .max_size(15)
        .min_idle(Some(5))
        .build(manager)
        .expect("Failed to create PostgreSQL pool");

    info!("PostgreSQL connection pool created successfully");

    pool
}

pub fn test_connection(pool: &DbPool) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = pool.get()?;

    diesel::sql_query("SELECT 1")
        .execute(&mut conn)?;

    info!("Database connection test passed");

    Ok(())
}

pub async fn start_db_processor(pool: DbPool) {
    let redis_url = env::var("REDIS_3_URL")
        .unwrap_or_else(|_| "redis://localhost:6382".to_string());

    info!("Connecting to Redis: {}", redis_url);

    let client = Client::open(redis_url.as_str())
        .expect("Failed to create Redis client");

    let mut conn = client
        .get_connection()
        .expect("Failed to connect to Redis");

    info!("DB processor started");

    loop {
        let result: Option<String> = redis::cmd("BRPOP")
            .arg("db_processor")
            .arg(1)
            .query(&mut conn)
            .unwrap_or(None);

        if let Some(message_str) = result {
            match serde_json::from_str::<DbMessage>(&message_str) {
                Ok(message) => {
                    info!("Received DB message");

                    if let Err(e) = process_message(message, &pool) {
                        error!("Failed to process message: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to deserialize message: {}", e);
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

fn process_message(
    message: DbMessage,
    pool: &DbPool,
) -> Result<(), diesel::result::Error> {
    let conn = &mut pool
        .get()
        .expect("Failed to get database connection");

    match message {
        DbMessage::TradeAdded(trade_message) => {
            info!("Processing trade {}", trade_message.id);

            if let Err(e) = trade_message.validate() {
                warn!(
                    "Trade validation failed for {}: {:?}",
                    trade_message.id,
                    e
                );
                return Ok(());
            }

            let trade = Trade {
                id: uuid::Uuid::parse_str(&trade_message.id)
                    .expect("Invalid trade UUID"),

                is_buyer_maker: trade_message.is_buyer_maker,

                price: trade_message.price,
                quantity: trade_message.quantity,
                quote_quantity: trade_message.quote_quantity,

                timestamp: Utc
                    .timestamp_opt(trade_message.timestamp, 0)
                    .unwrap()
                    .naive_utc(),

                market: trade_message.market,
            };

            diesel::insert_into(trades::table)
                .values(&trade)
                .execute(conn)?;

            info!("Trade inserted successfully");
        }

        DbMessage::OrderUpdate(order_message) => {
            info!("Processing order {}", order_message.order_id);

            if let Err(e) = order_message.validate() {
                warn!(
                    "Order validation failed for {}: {:?}",
                    order_message.order_id,
                    e
                );
                return Ok(());
            }

            let order = Order {
                id: uuid::Uuid::parse_str(&order_message.order_id)
                    .expect("Invalid order UUID"),

                executed_qty: order_message
                    .executed_qty
                    .to_string()
                    .parse()
                    .unwrap(),

                market: order_message.market.unwrap_or_default(),
                price: order_message.price.unwrap_or_default(),
                quantity: order_message.quantity.unwrap_or_default(),
                side: order_message.side.unwrap_or_default(),

                created_at: Utc::now().naive_utc(),
            };

            diesel::insert_into(orders::table)
                .values(&order)
                .execute(conn)?;

            info!("Order inserted successfully");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::RunQueryDsl;

    #[test]
    fn database_connection_works() {
        let pool = establish_connection_pool();

        let mut conn = pool
            .get()
            .expect("Failed to get connection");

        diesel::sql_query("SELECT 1")
            .execute(&mut conn)
            .expect("Query failed");

        println!("Database connection successful");
    }
}