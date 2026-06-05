




use diesel::prelude::*;
use uuid::Uuid;
use chrono::Utc;

use database::{
    models::User,
    schema::users::dsl::{users, email},
    DbPool,
};

use crate::{
    types::auth_types::{LoginForm, SignupForm},
    utils::jwt::generate_token,
};

use crate::utils::hashpassword::{
    hash_password,
    verify_password,
};

#[derive(Debug)]
pub enum AuthError {
    UserAlreadyExists,
    UserNotFound,
    InvalidCredentials,
    Database,
    Hash,
    Jwt,
}

pub struct AuthService;

impl AuthService {
    pub async fn signup(
        pool: DbPool,
        form: SignupForm,
        jwt_secret: &str,
    ) -> Result<String, AuthError> {
        let password = form.password.clone();

        let hashed_password = tokio::task::spawn_blocking(move || {
            hash_password(&password)
        })
        .await
        .map_err(|_| AuthError::Hash)?
        .map_err(|_| AuthError::Hash)?;

        let email_value = form.email.clone();
        let username_value = form.username.clone();

        let user_id = tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|_| AuthError::Database)?;

            let existing = users
                .filter(email.eq(&email_value))
                .first::<User>(&mut conn)
                .optional()
                .map_err(|_| AuthError::Database)?;

            if existing.is_some() {
                return Err(AuthError::UserAlreadyExists);
            }

            let user = User {
                id: Uuid::new_v4(),
                email: email_value,
                username: username_value,
                password_hash: hashed_password,
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            };

            diesel::insert_into(users)
                .values(&user)
                .execute(&mut conn)
                .map_err(|_| AuthError::Database)?;

            Ok::<Uuid, AuthError>(user.id)
        })
        .await
        .map_err(|_| AuthError::Database)??;

        let token = generate_token(
            user_id.to_string(),
            jwt_secret,
        )
        .map_err(|_| AuthError::Jwt)?;

        Ok(token)
    }

    pub async fn login(
        pool: DbPool,
        form: LoginForm,
        jwt_secret: &str,
    ) -> Result<String, AuthError> {
        let email_value = form.email.clone();

        let user = tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|_| AuthError::Database)?;

            users
                .filter(email.eq(&email_value))
                .first::<User>(&mut conn)
                .optional()
                .map_err(|_| AuthError::Database)?
                .ok_or(AuthError::UserNotFound)
        })
        .await
        .map_err(|_| AuthError::Database)??;

        let password = form.password.clone();
        let stored_hash = user.password_hash.clone();

        let valid = tokio::task::spawn_blocking(move || {
            verify_password(
                &password,
                &stored_hash,
            )
        })
        .await
        .map_err(|_| AuthError::Hash)?
        .map_err(|_| AuthError::Hash)?;

        if !valid {
            return Err(AuthError::InvalidCredentials);
        }

        let token = generate_token(
            user.id.to_string(),
            jwt_secret,
        )
        .map_err(|_| AuthError::Jwt)?;

        Ok(token)
    }
}