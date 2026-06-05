// use redis::{Client, RedisResult, Commands};
// use once_cell::sync::Lazy;
// use crate::types::redis::{MessageFromOrderbook, MessageToEngine};
// use serde::Serialize;
// use rand::{thread_rng, Rng};
// use rand::distributions::Alphanumeric;
// use log::info;

// #[derive(Serialize, Debug)]
// struct MessageWrapper {
//     client_id: String,
//     user_id: String,
//     message: MessageToEngine,
// }

// static INSTANCE: Lazy<RedisManager> = Lazy::new(|| {
//     RedisManager::new()
// });

// pub struct RedisManager {
//     client: Client,
//     publisher: Client,
// }

// impl RedisManager {
//     fn new() -> Self {
//         let client = Client::open("redis://localhost:6379").unwrap();
//         let publisher = Client::open("redis://localhost:6379").unwrap();
//         RedisManager { client, publisher }
//     }

//   pub fn get_instance() -> &'static RedisManager {
//     &INSTANCE
// }

//     fn get_random_client_id(&self) -> String {
//         thread_rng()
//             .sample_iter(&Alphanumeric)
//             .take(26) 
//             .map(char::from)
//             .collect()
//     }

//     pub async fn send_and_await(&self, message: MessageToEngine, user_id: String) -> RedisResult<MessageFromOrderbook> {
//         info!("Sending message to engine: {:?}", message);
//         let mut conn = self.client.get_connection()?;
//         let mut pub_conn = self.publisher.get_connection()?;
//         info!("Connected to Redis");
//         let client_id = self.get_random_client_id();
//         info!("Client ID: {:?}", client_id);

//         let mut pubsub = conn.as_pubsub();
//         info!("Created PubSub");
//         pubsub.subscribe(&client_id)?;
//         info!("Subscribed to channel: {}", client_id);

//         let message_wrapper = MessageWrapper {
//             client_id: client_id.clone(),
//             user_id,
//             message,
//         };
//         info!("Message with ID: {:?}", message_wrapper);
//         let _: () = pub_conn.lpush("messages", serde_json::to_string(&message_wrapper).expect("Failed to serialize message"))?;
//         info!("Pushed message to Redis");
//         info!("Waiting for response on channel: {}", client_id);
//         let msg = pubsub.get_message()?;
//         info!("Received message from Redis: {:?}", msg);
//         let payload: String = msg.get_payload()?;
//         info!("Received message from Redis: {:?}", payload);
//         pubsub.unsubscribe(&client_id)?;
//         info!("Unsubscribed from client ID: {:?}", client_id);
//         Ok(serde_json::from_str(&payload).expect("Failed to deserialize message"))
//     }
// }















use redis::{Client, Commands, RedisResult};
use once_cell::sync::Lazy;

use crate::types::redis::{MessageFromOrderbook, MessageToEngine};

use serde::Serialize;

use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

use log::{error, info};

/// ---------------------------------------------------------------------------
/// Message sent to the engine through Redis.
///
/// Structure:
/// {
///     client_id: "<temporary reply channel>",
///     user_id: "<user making request>",
///     message: { ... actual message ... }
/// }
///
/// The engine processes the message and publishes the response
/// back to the `client_id` channel.
/// ---------------------------------------------------------------------------
#[derive(Serialize, Debug)]
struct MessageWrapper {
    client_id: String,
    user_id: String,
    message: MessageToEngine,
}

/// ---------------------------------------------------------------------------
/// Singleton RedisManager instance.
///
/// This allows the application to use:
///
/// RedisManager::get_instance()
///
/// from anywhere without creating multiple managers.
/// ---------------------------------------------------------------------------
static INSTANCE: Lazy<RedisManager> = Lazy::new(|| {
    println!("[REDIS_MANAGER] Initializing singleton instance...");
    RedisManager::new()
});

/// ---------------------------------------------------------------------------
/// RedisManager
///
/// client    -> used for PubSub subscriptions
/// publisher -> used for LPUSH / publishing requests
/// ---------------------------------------------------------------------------
pub struct RedisManager {
    client: Client,
    publisher: Client,
}

impl RedisManager {
    /// -----------------------------------------------------------------------
    /// Create a new RedisManager.
    ///
    /// Creates two Redis clients:
    /// 1. Subscriber connection
    /// 2. Publisher connection
    ///
    /// Using separate clients avoids blocking issues with PubSub.
    /// -----------------------------------------------------------------------
    fn new() -> Self {
        println!("\n==================================================");
        println!("Initializing Redis Manager");
        println!("==================================================");

        info!("Creating Redis clients...");

        let redis_url = "redis://localhost:6380";

        println!("Connecting to Redis at: {}", redis_url);

        let client =
            Client::open(redis_url).expect("Failed to create Redis subscriber client");

        let publisher =
            Client::open(redis_url).expect("Failed to create Redis publisher client");

        println!("Redis clients created successfully");
        info!("Redis clients initialized");

        RedisManager { client, publisher }
    }

    /// -----------------------------------------------------------------------
    /// Returns singleton RedisManager instance.
    /// -----------------------------------------------------------------------
    pub fn get_instance() -> &'static RedisManager {
        &INSTANCE
    }

    /// -----------------------------------------------------------------------
    /// Generate a random client ID.
    ///
    /// This ID acts as:
    /// - Temporary response channel
    /// - Correlation identifier
    ///
    /// Example:
    ///     Ah83KdP92LmNxQwErTyUiOpZ12
    /// -----------------------------------------------------------------------
    fn get_random_client_id(&self) -> String {
        let id: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(26)
            .map(char::from)
            .collect();

        println!("[CLIENT_ID_GENERATED] {}", id);

        id
    }

    /// -----------------------------------------------------------------------
    /// Flow:
    ///
    /// 1. Connect to Redis
    /// 2. Verify connection with PING
    /// 3. Generate temporary reply channel
    /// 4. Subscribe to reply channel
    /// 5. Push request into Redis queue
    /// 6. Wait for engine response
    /// 7. Receive response
    /// 8. Unsubscribe
    /// 9. Deserialize and return
    /// -----------------------------------------------------------------------
    pub async fn send_and_await(
        &self,
        message: MessageToEngine,
        user_id: String,
    ) -> RedisResult<MessageFromOrderbook> {
        println!("\n");
        println!("==================================================");
        println!("STARTING REQUEST");
        println!("==================================================");

        info!("Sending message to engine");

        println!("Incoming User ID: {}", user_id);
        println!("Message Payload: {:?}", message);

        // --------------------------------------------------------------------
        // STEP 1: Create Redis connections
        // --------------------------------------------------------------------
        println!("\n[STEP 1] Creating Redis connections...");

        let mut conn = self.client.get_connection()?;
        let mut pub_conn = self.publisher.get_connection()?;

        println!("Subscriber connection established");
        println!("Publisher connection established");

        info!("Redis connections established");

        // --------------------------------------------------------------------
        // STEP 2: Verify Redis connectivity
        // --------------------------------------------------------------------
        println!("\n[STEP 2] Performing Redis health check (PING)...");

        let ping_response: String = redis::cmd("PING").query(&mut pub_conn)?;

        println!("PING -> {}", ping_response);
        info!("Redis PING response: {}", ping_response);

        // --------------------------------------------------------------------
        // STEP 3: Generate unique client channel
        // --------------------------------------------------------------------
        println!("\n[STEP 3] Generating temporary response channel...");

        let client_id = self.get_random_client_id();

        println!("Temporary Reply Channel: {}", client_id);
        info!("Generated client channel: {}", client_id);

        // --------------------------------------------------------------------
        // STEP 4: Subscribe to channel
        // --------------------------------------------------------------------
        println!("\n[STEP 4] Creating PubSub connection...");

        let mut pubsub = conn.as_pubsub();

        println!("PubSub object created");

        pubsub.subscribe(&client_id)?;

        println!("Subscribed successfully");
        println!("Listening on channel: {}", client_id);

        info!("Subscribed to channel {}", client_id);

        // --------------------------------------------------------------------
        // STEP 5: Wrap outgoing message
        // --------------------------------------------------------------------
        println!("\n[STEP 5] Wrapping outgoing message...");

        let message_wrapper = MessageWrapper {
            client_id: client_id.clone(),
            user_id,
            message,
        };

        println!("Wrapped Message:");
        println!("{:#?}", message_wrapper);

        info!("Message wrapper created");

        // --------------------------------------------------------------------
        // STEP 6: Serialize message
        // --------------------------------------------------------------------
        println!("\n[STEP 6] Serializing message...");

        let serialized_message = serde_json::to_string(&message_wrapper)
            .expect("Failed to serialize message");

        println!("Serialized JSON:");
        println!("{}", serialized_message);

        // --------------------------------------------------------------------
        // STEP 7: Push message into queue
        // --------------------------------------------------------------------
        println!("\n[STEP 7] Pushing message into Redis queue...");

        let _: () = pub_conn.lpush("messages", &serialized_message)?;

        println!("Message pushed successfully");
        println!("Queue Name: messages");

        info!("Message pushed to Redis queue");

        // --------------------------------------------------------------------
        // STEP 8: Wait for engine response
        // --------------------------------------------------------------------
        println!("\n[STEP 8] Waiting for engine response...");
        println!("Listening on channel: {}", client_id);

        info!("Waiting for response");

        let msg = pubsub.get_message()?;

        println!("Response received from Redis");

        // --------------------------------------------------------------------
        // STEP 9: Extract payload
        // --------------------------------------------------------------------
        println!("\n[STEP 9] Extracting payload...");

        let payload: String = msg.get_payload()?;

        println!("Raw Payload:");
        println!("{}", payload);

        info!("Payload received: {}", payload);

        // --------------------------------------------------------------------
        // STEP 10: Cleanup subscription
        // --------------------------------------------------------------------
        println!("\n[STEP 10] Cleaning up PubSub subscription...");

        pubsub.unsubscribe(&client_id)?;

        println!("Unsubscribed from channel");
        println!("Channel: {}", client_id);

        info!("Unsubscribed from {}", client_id);

        // --------------------------------------------------------------------
        // STEP 11: Deserialize response
        // --------------------------------------------------------------------
        println!("\n[STEP 11] Deserializing response...");

        let response: MessageFromOrderbook =
            serde_json::from_str(&payload)
                .expect("Failed to deserialize response");

        println!("Parsed Response:");
        println!("{:#?}", response);

        info!("Response successfully deserialized");

        println!("\n==================================================");
        println!("REQUEST COMPLETED SUCCESSFULLY");
        println!("==================================================");

        Ok(response)
    }
}