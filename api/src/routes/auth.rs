
use axum::{
    routing::{get, post},
    Router,
};

use crate::{
    controller::auth_controller,
    AppState,
};

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/signup", post(auth_controller::signup))
        .route("/login", post(auth_controller::login))
        .route("/logout", post(auth_controller::logout))
        .route("/check-auth", get(auth_controller::check_auth))
}