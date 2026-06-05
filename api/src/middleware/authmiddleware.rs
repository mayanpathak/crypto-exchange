use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use axum_extra::extract::cookie::CookieJar;

use serde_json::json;

use crate::utils::jwt::verify_token;

pub async fn auth_middleware(
    jar: CookieJar,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    // Read JWT secret
    let jwt_secret = std::env::var("JWT_SECRET")
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Server configuration error"
                })),
            )
                .into_response()
        })?;

    // Read token from cookie
    let token = match jar.get("token") {
        Some(cookie) => cookie.value().to_string(),
        None => {
            return Err(
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "success": false,
                        "message": "Authentication required"
                    })),
                )
                    .into_response(),
            );
        }
    };

    // Verify JWT
    let claims = match verify_token(&token, &jwt_secret) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "success": false,
                        "message": "Invalid or expired token"
                    })),
                )
                    .into_response(),
            );
        }
    };

    // Store authenticated user id in request extensions
    req.extensions_mut()
        .insert(claims.user_id);

    // Continue request pipeline
    Ok(next.run(req).await)
}