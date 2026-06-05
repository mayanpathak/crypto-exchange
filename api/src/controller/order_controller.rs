use axum::{
    extract::{Extension, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
       
};

use crate::services::order_services::OrderService;
use crate::types::redis::{
    CancelOrderData,
    CreateOrderData,
};

#[derive(serde::Deserialize)]
pub struct OpenOrdersQuery {
    pub market: String,
}

// pub async fn get_open_orders(
//     Extension(user_id): Extension<String>,
//     Query(query): Query<OpenOrdersQuery>,
// ) -> impl IntoResponse {
//     match OrderService::get_open_orders(
//         user_id,
//         query.market,
//     )
//     .await
//     {
//         Ok(response) => (
//             StatusCode::OK,
//             Json(response),
//         )
//             .into_response(),

//         Err(_) => StatusCode::INTERNAL_SERVER_ERROR
//             .into_response(),
//     }
// }



// pub async fn get_open_orders(
//     Extension(user_id): Extension<String>,
//     Query(query): Query<OpenOrdersQuery>,
// ) -> impl IntoResponse {
//     match OrderService::get_open_orders(
//         user_id,
//         query.market,
//     )
//     .await
//     {
//         Ok(response) => (
//             StatusCode::OK,
//             Json(response),
//         )
//             .into_response(),
        
//         Err(e) => {
//             // Log the actual error
//             eprintln!("❌ Failed to get open orders: {:?}", e);
//             (
//                 StatusCode::INTERNAL_SERVER_ERROR,
//                 Json(serde_json::json!({
//                     "error": format!("Failed to get open orders: {}", e)
//                 })),
//             ).into_response()
//         }
//     }
// }







pub async fn get_open_orders(
    Extension(user_id): Extension<String>,
    Query(query): Query<OpenOrdersQuery>,
) -> impl IntoResponse {
    match OrderService::get_open_orders(
        user_id,
        query.market,
    )
    .await
    {
        Ok(response) => (
            StatusCode::OK,
            Json(response),
        )
            .into_response(),
        
        Err(e) => {
            // Log the actual error
            eprintln!("❌ Failed to get open orders: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to get open orders: {}", e)
                })),
            ).into_response()
        }
    }
}

























pub async fn cancel_order(
    Extension(user_id): Extension<String>,
    Json(body): Json<CancelOrderData>,  // ← Change Query to Json
) -> impl IntoResponse {
    match OrderService::cancel_order(
        user_id,
        body,  // ← Change query to body
    )
    .await
    {
        Ok(response) => (
            StatusCode::OK,
            Json(response),
        )
            .into_response(),

        Err(e) => {
            eprintln!("❌ Failed to cancel order: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to cancel order: {}", e)
                })),
            ).into_response()
        }
    }
}










pub async fn create_order(
    Extension(user_id): Extension<String>,
    Json(body): Json<CreateOrderData>,
) -> impl IntoResponse {
    match OrderService::create_order(
        user_id,
        body,
    )
    .await
    {
        Ok(response) => (
            StatusCode::OK,
            Json(response),
        )
            .into_response(),

        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create order",
        )
            .into_response(),
    }
}