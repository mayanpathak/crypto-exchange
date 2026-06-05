use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use axum_extra::extract::cookie::CookieJar;

use serde_json::json;
use validator::Validate;

use crate::{
    AppState,
    services::auth_services::{
        AuthError,
        AuthService,
    },
    types::auth_types::{
        LoginForm,
        SignupForm,
    },
    utils::{
        cookie::{
            build_auth_cookie,
            clear_auth_cookie,
        },
        jwt::verify_token,
    },
};

pub async fn signup(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(form): Json<SignupForm>,
) -> impl IntoResponse {
    if let Err(err) = form.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!(err)),
        )
            .into_response();
    }

   

    match AuthService::signup(
        state.db_pool.clone(),
        form,
        &state.jwt_secret,
    )
    .await
    {
        Ok(token) => (
            StatusCode::CREATED,
            jar.add(build_auth_cookie(token)),
            Json(json!({
                "success": true,
                "message": "Signup successful"
            })),
        )
            .into_response(),

        Err(AuthError::UserAlreadyExists) => (
            StatusCode::CONFLICT,
            Json(json!({
                "success": false,
                "message": "User already exists"
            })),
        )
            .into_response(),

        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": "Internal server error"
            })),
        )
            .into_response(),
    }
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(form): Json<LoginForm>,
) -> impl IntoResponse {
    if let Err(err) = form.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!(err)),
        )
            .into_response();
    }

   
    match AuthService::login(
        state.db_pool.clone(),
        form,
        &state.jwt_secret,
    )
    .await
    {
        Ok(token) => (
            StatusCode::OK,
            jar.add(build_auth_cookie(token)),
            Json(json!({
                "success": true,
                "message": "Login successful"
            })),
        )
            .into_response(),

        Err(AuthError::UserNotFound   
        ) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "success": false,
                "message": "User not found"
            })),
        )
            .into_response(),

        Err(AuthError::InvalidCredentials) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "success": false,
                "message": "Invalid credentials"
            })),
        )
            .into_response(),

        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": "Internal server error"
            })),
        )
            .into_response(),
    }
}

pub async fn logout(
    jar: CookieJar,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        jar.add(clear_auth_cookie()),
        Json(json!({
            "success": true,
            "message": "Logout successful"
        })),
    )
}

pub async fn check_auth(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let token = match jar.get("token") {
        Some(token) => token.value().to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "success": false,
                    "message": "Not authenticated"
                })),
            )
                .into_response();
        }
    };

    match verify_token(&token, &state.jwt_secret) {
        Ok(claims) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "user_id": claims.user_id
            })),
        )
            .into_response(),

        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "success": false,
                "message": "Invalid token"
            })),
        )
            .into_response(),
    }
}