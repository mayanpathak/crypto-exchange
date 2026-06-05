use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use log::info;

use crate::services::depth_services::fetch_depth;

#[derive(serde::Deserialize)]
pub struct MarketPath {
    pub market: String,
}

pub async fn get_depth(
    Extension(user_id): Extension<String>,
    Path(path): Path<MarketPath>,
) -> impl IntoResponse {
    info!("Getting depth for market: {}", path.market);

    match fetch_depth(path.market, user_id).await {
        Ok(response) => (
            StatusCode::OK,
            Json(response),
        )
            .into_response(),

        Err(_) => StatusCode::INTERNAL_SERVER_ERROR
            .into_response(),
    }
}