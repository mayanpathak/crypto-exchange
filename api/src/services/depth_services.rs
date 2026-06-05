


use crate::{
    redis::redis_manager::RedisManager,
    types::redis::{
        GetDepthData,
        MessageToEngine,
        MessageFromOrderbook,
    },
};

pub async fn fetch_depth(
    market: String,
    user_id: String,
) -> Result<MessageFromOrderbook, Box<dyn std::error::Error>> {
    let redis_manager = RedisManager::get_instance();

    let message = MessageToEngine::GetDepth {
        data: GetDepthData {
            market,
        },
    };

    let response = redis_manager
        .send_and_await(message, user_id)
        .await?;

    Ok(response)
}