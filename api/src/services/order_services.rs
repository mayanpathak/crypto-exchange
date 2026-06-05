use crate::redis::redis_manager::RedisManager;
use crate::types::redis::{
    MessageToEngine,
    GetOpenOrdersData,
    CancelOrderData,
    CreateOrderData,
};


use crate::types::redis::MessageFromOrderbook;

pub struct OrderService;

impl OrderService {
    pub async fn get_open_orders(
    user_id: String,
    market: String,
) -> Result<MessageFromOrderbook, redis::RedisError> {
        let redis_manager = RedisManager::get_instance();
            

        let message = MessageToEngine::GetOpenOrders {
            data: GetOpenOrdersData { market,user_id: user_id.clone() },
        };

        let response = redis_manager
            .send_and_await(message, user_id)
            .await?;

        Ok(response)
    }

   pub async fn cancel_order(
    user_id: String,
    data: CancelOrderData,
) -> Result<MessageFromOrderbook, redis::RedisError>{
        let redis_manager = RedisManager::get_instance()
            ;

        let message = MessageToEngine::CancelOrder {
            data,
            
            
        };

        let response = redis_manager
            .send_and_await(message, user_id)
            .await?;

        Ok(response)
    }

pub async fn create_order(
    user_id: String,
    data: CreateOrderData,
) -> Result<MessageFromOrderbook, redis::RedisError> {
        let redis_manager = RedisManager::get_instance();

        let message = MessageToEngine::CreateOrder {
            data,
        };

        let response = redis_manager
            .send_and_await(message, user_id)
            .await?;

        Ok(response)
    }
}