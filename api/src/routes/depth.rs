use axum::{
    middleware,
    routing::get,
    Router,
};

use crate::{
    controller::depth_controller::get_depth,
    middleware::auth_middleware,
    AppState,
};

pub fn depth_routes() -> Router<AppState> {
    Router::new()
.route("/{market}", get(get_depth))        .route_layer(middleware::from_fn(auth_middleware))
}