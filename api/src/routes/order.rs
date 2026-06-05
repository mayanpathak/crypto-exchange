
use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};

use crate::{
    controller::order_controller::{cancel_order, create_order, get_open_orders},
    middleware::auth_middleware,
    AppState,
};

pub fn order_routes() -> Router<AppState> {
    Router::new()
        .route("/open", get(get_open_orders))
        .route("/", post(create_order).delete(cancel_order))
        .route_layer(middleware::from_fn(auth_middleware))
}