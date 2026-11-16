use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::str::FromStr;

use crate::{ApiError, AppState};

const SESSION_COOKIE: &str = "aurelia_session";
const SESSION_TTL_SECONDS: i64 = 43_200;

#[derive(Deserialize)]
pub(crate) struct LoginRequest {
    pub(crate) username: String,
    pub(crate) password: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct StaffIdentity {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) role: String,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
}

pub(crate) fn normalize_username(raw: &str) -> Result<String, String> {
    let username = raw.trim().to_ascii_lowercase();
    let bytes = username.as_bytes();
    if !(3..=32).contains(&bytes.len())
        || !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        || !bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(*byte, b'.' | b'_' | b'-')
        })
    {
        return Err(
            "username must be 3-32 lowercase ASCII letters, digits, dots, underscores or hyphens"
                .to_owned(),
        );
    }
    Ok(username)
}

pub(crate) fn validate_password(password: &str) -> Result<(), String> {
    if !(12..=128).contains(&password.len()) {
        return Err("password must contain between 12 and 128 bytes".to_owned());
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, String> {
    let mut rng = OsRng;
    let salt = SaltString::generate(&mut rng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "password hashing failed".to_owned())
}

fn verify_password(password: &str, encoded_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub(crate) async fn bootstrap_owner(
    db: &PgPool,
    raw_username: &str,
    password: &str,
) -> Result<(), String> {
    let username = normalize_username(raw_username)?;
    validate_password(password)?;
    let password_hash = hash_password(password)?;

    let mut tx = db
        .begin()
        .await
        .map_err(|_| "could not start owner bootstrap transaction".to_owned())?;

    let _: bool = sqlx::query_scalar("SELECT pg_advisory_xact_lock($1) IS NULL")
        .bind(0x4155_5245_4C49_41_i64)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "could not lock owner bootstrap".to_owned())?;

    let has_staff: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM staff_users)")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "could not inspect existing staff accounts".to_owned())?;

    if has_staff {
        return Err(
            "bootstrap-owner is disabled after the first staff account has been created"
                .to_owned(),
        );
    }

    sqlx::query(
        "INSERT INTO staff_users (username, password_hash, role)
         VALUES ($1, $2, 'owner')",
    )
    .bind(username)
    .bind(password_hash)
    .execute(&mut *tx)
    .await
    .map_err(|_| "could not create the initial owner account".to_owned())?;

    tx.commit()
        .await
        .map_err(|_| "could not commit owner bootstrap transaction".to_owned())
}

pub(crate) async fn create_staff(
    db: &PgPool,
    raw_username: &str,
    password: &str,
    role: &str,
) -> Result<(), String> {
    let username = normalize_username(raw_username)?;
    validate_password(password)?;
    if !matches!(role, "manager" | "cashier" | "chef" | "waiter") {
        return Err("staff role must be manager, cashier, chef, or waiter".to_owned());
    }
    let password_hash = hash_password(password)?;

    let mut tx = db
        .begin()
        .await
        .map_err(|_| "could not start staff creation transaction".to_owned())?;

    let has_owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM staff_users
            WHERE role = 'owner' AND is_active = TRUE
        )",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| "could not verify an active owner account".to_owned())?;

    if !has_owner {
        return Err("an active owner account is required before creating staff".to_owned());
    }

    sqlx::query(
        "INSERT INTO staff_users (username, password_hash, role)
         VALUES ($1, $2, $3)",
    )
    .bind(username)
    .bind(password_hash)
    .bind(role)
    .execute(&mut *tx)
    .await
    .map_err(|_| "could not create staff account; username may already exist".to_owned())?;

    tx.commit()
        .await
        .map_err(|_| "could not commit staff creation transaction".to_owned())
}

fn hash_session_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            if name == SESSION_COOKIE
                && value.len() == 64
                && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                Some(value)
            } else {
                None
            }
        })
}

fn session_cookie(token: &str, secure: bool) -> String {
    let secure_attribute = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECONDS}{secure_attribute}"
    )
}

fn expired_session_cookie(secure: bool) -> String {
    let secure_attribute = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_attribute}"
    )
}

fn set_no_store(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
}

pub(crate) async fn current_staff(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<StaffIdentity, ApiError> {
    let token = session_token(headers).ok_or(ApiError::Unauthorized)?;
    let token_hash = hash_session_token(token);

    let staff = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT u.id, u.username, u.role
         FROM staff_sessions s
         INNER JOIN staff_users u ON u.id = s.staff_user_id
         WHERE s.token_hash = $1
           AND s.revoked_at IS NULL
           AND s.expires_at > now()
           AND u.is_active = TRUE",
    )
    .bind(token_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?
    .map(|(id, username, role)| StaffIdentity { id, username, role })
    .ok_or(ApiError::Unauthorized)?;

    Ok(staff)
}

pub(crate) fn can_view_admin_orders(role: &str) -> bool {
    matches!(role, "owner" | "manager" | "cashier")
}

pub(crate) fn can_view_kitchen_orders(role: &str) -> bool {
    matches!(role, "owner" | "manager" | "chef")
}

pub(crate) fn can_view_service_orders(role: &str) -> bool {
    matches!(role, "owner" | "manager" | "waiter")
}

pub(crate) async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let username = normalize_username(&payload.username).map_err(|_| ApiError::Unauthorized)?;
    if payload.password.len() > 128 {
        return Err(ApiError::Unauthorized);
    }

    let user = sqlx::query_as::<_, (i64, String, String, bool, String)>(
        "SELECT id, username, password_hash, is_active, role
         FROM staff_users
         WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    let Some((user_id, username, password_hash, is_active, role)) = user else {
        // Spend a password-hash operation on unknown usernames to reduce timing-based
        // account enumeration. The gateway also applies a stricter per-IP login limit.
        let _ = hash_password(&payload.password);
        return Err(ApiError::Unauthorized);
    };

    if !verify_password(&payload.password, &password_hash) || !is_active {
        return Err(ApiError::Unauthorized);
    }

    sqlx::query(
        "DELETE FROM staff_sessions
         WHERE expires_at <= now() OR revoked_at IS NOT NULL",
    )
    .execute(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    let mut random_bytes = [0_u8; 32];
    let mut rng = OsRng;
    rng.fill_bytes(&mut random_bytes);
    let token = hex::encode(random_bytes);
    let token_hash = hash_session_token(&token);

    sqlx::query(
        "INSERT INTO staff_sessions (token_hash, staff_user_id, expires_at)
         VALUES ($1, $2, now() + ($3 * interval '1 second'))",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(SESSION_TTL_SECONDS)
    .execute(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    let identity = StaffIdentity {
        id: user_id,
        username,
        role,
    };

    let cookie = session_cookie(&token, state.cookie_secure);
    let mut response = (StatusCode::OK, Json(identity)).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|_| ApiError::Internal)?,
    );
    set_no_store(&mut response);
    Ok(response)
}

pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = session_token(&headers) {
        sqlx::query(
            "UPDATE staff_sessions
             SET revoked_at = COALESCE(revoked_at, now())
             WHERE token_hash = $1",
        )
        .bind(hash_session_token(token))
        .execute(&state.db)
        .await
        .map_err(|_| ApiError::Internal)?;
    }

    let cookie = expired_session_cookie(state.cookie_secure);
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|_| ApiError::Internal)?,
    );
    set_no_store(&mut response);
    Ok(response)
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let identity = current_staff(&state, &headers).await?;
    let mut response = Json(identity).into_response();
    set_no_store(&mut response);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_normalization_is_strict_and_case_insensitive() {
        assert_eq!(normalize_username("  Owner_01 ").unwrap(), "owner_01");
        assert!(normalize_username("ab").is_err());
        assert!(normalize_username("bad name").is_err());
        assert!(normalize_username("نام").is_err());
    }

    #[test]
    fn passwords_have_a_bounded_length() {
        assert!(validate_password("correct-horse-12").is_ok());
        assert!(validate_password("short").is_err());
        assert!(validate_password(&"x".repeat(129)).is_err());
    }

    #[test]
    fn role_permissions_are_separated_by_work_area() {
        assert!(can_view_admin_orders("owner"));
        assert!(can_view_admin_orders("manager"));
        assert!(can_view_admin_orders("cashier"));
        assert!(!can_view_admin_orders("chef"));
        assert!(!can_view_admin_orders("waiter"));

        assert!(can_view_kitchen_orders("chef"));
        assert!(!can_view_kitchen_orders("cashier"));

        assert!(can_view_service_orders("waiter"));
        assert!(!can_view_service_orders("chef"));
    }

    #[test]
    fn session_cookies_use_secure_attributes_by_default() {
        let cookie = session_cookie("0123456789abcdef", true);
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("; Secure"));
        assert!(cookie.contains("Max-Age=43200"));

        let local_cookie = session_cookie("0123456789abcdef", false);
        assert!(!local_cookie.contains("; Secure"));
    }

    #[test]
    fn session_cookie_parser_rejects_malformed_values() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=value; aurelia_session=not-a-token"),
        );
        assert!(session_token(&headers).is_none());

        headers.insert(
            header::COOKIE,
            HeaderValue::from_static(
                "aurelia_session=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
        );
        assert_eq!(
            session_token(&headers),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
    }
}
