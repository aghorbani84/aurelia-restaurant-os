mod auth;

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{env, net::SocketAddr};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
struct AppState {
    db: PgPool,
    cookie_secure: bool,
    csrf_allowed_origins: Vec<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
}

#[derive(Debug)]
enum ApiError {
    BadRequest(&'static str),
    Unauthorized,
    Forbidden,
    NotFound(&'static str),
    Conflict(&'static str),
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "invalid credentials or session"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "insufficient permissions"),
            Self::NotFound(message) => (StatusCode::NOT_FOUND, message),
            Self::Conflict(message) => (StatusCode::CONFLICT, message),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal server error"),
        };
        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

#[derive(Serialize)]
struct MenuCategory {
    id: i64,
    name: String,
    slug: String,
}

#[derive(Serialize)]
struct MenuItem {
    id: i64,
    category_id: Option<i64>,
    category_name: Option<String>,
    name: String,
    description: String,
    price_minor: i64,
    available: bool,
}

#[derive(Deserialize)]
struct CreateOrderRequest {
    customer_name: String,
    items: Vec<OrderItemInput>,
}

#[derive(Deserialize)]
struct OrderItemInput {
    menu_item_id: i64,
    quantity: i32,
}

#[derive(Serialize)]
struct CreatedOrder {
    id: i64,
    order_number: String,
    status: String,
    total_minor: i64,
}

#[derive(Serialize)]
struct OrderSummary {
    id: i64,
    order_number: String,
    customer_name: String,
    status: String,
    total_minor: i64,
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
    before_id: Option<i64>,
}

#[derive(Serialize)]
struct AuditEvent {
    id: i64,
    event_type: String,
    attempted_username: Option<String>,
    actor_staff_user_id: Option<i64>,
    actor_username: Option<String>,
    subject_staff_user_id: Option<i64>,
    subject_username: Option<String>,
    subject_role: Option<String>,
    created_at: String,
}

#[derive(Serialize)]
struct AuditPage {
    events: Vec<AuditEvent>,
    next_cursor: Option<i64>,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn readiness(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(HealthResponse { status: "ready" }))
}

async fn list_categories(
    State(state): State<AppState>,
) -> Result<Json<Vec<MenuCategory>>, ApiError> {
    let rows = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, name, slug FROM menu_categories ORDER BY name ASC LIMIT 200",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, name, slug)| MenuCategory { id, name, slug })
            .collect(),
    ))
}

async fn list_menu_items(State(state): State<AppState>) -> Result<Json<Vec<MenuItem>>, ApiError> {
    let rows = sqlx::query_as::<_, (i64, Option<i64>, Option<String>, String, String, i64, bool)>(
        "SELECT mi.id, mi.category_id, mc.name, mi.name, mi.description, mi.price_minor, mi.available
         FROM menu_items mi
         LEFT JOIN menu_categories mc ON mc.id = mi.category_id
         ORDER BY mi.name ASC
         LIMIT 500",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(id, category_id, category_name, name, description, price_minor, available)| {
                    MenuItem {
                        id,
                        category_id,
                        category_name,
                        name,
                        description,
                        price_minor,
                        available,
                    }
                },
            )
            .collect(),
    ))
}

async fn create_order(
    State(state): State<AppState>,
    Json(payload): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<CreatedOrder>), ApiError> {
    let customer_name = payload.customer_name.trim();
    if customer_name.is_empty() || customer_name.chars().count() > 120 {
        return Err(ApiError::BadRequest(
            "customer_name must contain between 1 and 120 characters",
        ));
    }
    if payload.items.is_empty() || payload.items.len() > 50 {
        return Err(ApiError::BadRequest(
            "items must contain between 1 and 50 entries",
        ));
    }
    if payload
        .items
        .iter()
        .any(|item| item.menu_item_id <= 0 || !(1..=99).contains(&item.quantity))
    {
        return Err(ApiError::BadRequest(
            "menu_item_id must be positive and quantity must be between 1 and 99",
        ));
    }

    let mut tx = state.db.begin().await.map_err(|_| ApiError::Internal)?;
    let mut total_minor: i64 = 0;
    let mut order_lines = Vec::with_capacity(payload.items.len());

    for item in &payload.items {
        let menu_item = sqlx::query_as::<_, (i64, bool)>(
            "SELECT price_minor, available FROM menu_items WHERE id = $1 FOR SHARE",
        )
        .bind(item.menu_item_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound("one or more menu items do not exist"))?;

        let (unit_price_minor, available) = menu_item;
        if !available {
            return Err(ApiError::Conflict("one or more menu items are unavailable"));
        }

        let line_total = unit_price_minor
            .checked_mul(i64::from(item.quantity))
            .ok_or(ApiError::BadRequest("order total is out of range"))?;
        total_minor = total_minor
            .checked_add(line_total)
            .ok_or(ApiError::BadRequest("order total is out of range"))?;
        order_lines.push((item.menu_item_id, item.quantity, unit_price_minor));
    }

    let order_number = format!("AUR-{}", Uuid::new_v4().simple());
    let (order_id, status) = sqlx::query_as::<_, (i64, String)>(
        "INSERT INTO orders (order_number, customer_name, status, total_minor)
         VALUES ($1, $2, 'pending', $3)
         RETURNING id, status",
    )
    .bind(&order_number)
    .bind(customer_name)
    .bind(total_minor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    for (menu_item_id, quantity, unit_price_minor) in order_lines {
        sqlx::query(
            "INSERT INTO order_lines (order_id, menu_item_id, quantity, unit_price_minor)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(order_id)
        .bind(menu_item_id)
        .bind(quantity)
        .bind(unit_price_minor)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::Internal)?;
    }

    tx.commit().await.map_err(|_| ApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(CreatedOrder {
            id: order_id,
            order_number,
            status,
            total_minor,
        }),
    ))
}

async fn fetch_orders(db: &PgPool, active_only: bool) -> Result<Response, ApiError> {
    let rows = if active_only {
        sqlx::query_as::<_, (i64, String, String, String, i64)>(
            "SELECT id, order_number, customer_name, status, total_minor
             FROM orders
             WHERE status IN ('pending', 'accepted', 'preparing', 'ready')
             ORDER BY created_at ASC
             LIMIT 200",
        )
        .fetch_all(db)
        .await
        .map_err(|_| ApiError::Internal)?
    } else {
        sqlx::query_as::<_, (i64, String, String, String, i64)>(
            "SELECT id, order_number, customer_name, status, total_minor
             FROM orders
             ORDER BY created_at DESC
             LIMIT 200",
        )
        .fetch_all(db)
        .await
        .map_err(|_| ApiError::Internal)?
    };

    let mut response = Json(
        rows.into_iter()
            .map(
                |(id, order_number, customer_name, status, total_minor)| OrderSummary {
                    id,
                    order_number,
                    customer_name,
                    status,
                    total_minor,
                },
            )
            .collect::<Vec<_>>(),
    )
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn list_admin_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if !auth::can_view_admin_orders(&staff.role) {
        return Err(ApiError::Forbidden);
    }
    fetch_orders(&state.db, false).await
}

async fn list_kitchen_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if !auth::can_view_kitchen_orders(&staff.role) {
        return Err(ApiError::Forbidden);
    }
    fetch_orders(&state.db, true).await
}

async fn list_service_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if !auth::can_view_service_orders(&staff.role) {
        return Err(ApiError::Forbidden);
    }
    fetch_orders(&state.db, true).await
}

async fn list_audit_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Result<(HeaderMap, Json<AuditPage>), ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if staff.role != "owner" {
        return Err(ApiError::Forbidden);
    }

    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || query.before_id.is_some_and(|id| id <= 0) {
        return Err(ApiError::BadRequest(
            "limit must be 1-100 and before_id must be positive",
        ));
    }

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            String,
        ),
    >(
        "SELECT log.id, log.event_type, log.attempted_username,
                log.actor_staff_user_id, actor.username,
                log.subject_staff_user_id, subject.username,
                log.subject_role, log.created_at::text
         FROM staff_audit_log log
         LEFT JOIN staff_users actor ON actor.id = log.actor_staff_user_id
         LEFT JOIN staff_users subject ON subject.id = log.subject_staff_user_id
         WHERE ($1::bigint IS NULL OR log.id < $1)
         ORDER BY log.id DESC
         LIMIT $2",
    )
    .bind(query.before_id)
    .bind(limit + 1)
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    let has_more = rows.len() > limit as usize;
    let events: Vec<AuditEvent> = rows
        .into_iter()
        .take(limit as usize)
        .map(
            |(
                id,
                event_type,
                attempted_username,
                actor_staff_user_id,
                actor_username,
                subject_staff_user_id,
                subject_username,
                subject_role,
                created_at,
            )| AuditEvent {
                id,
                event_type,
                attempted_username,
                actor_staff_user_id,
                actor_username,
                subject_staff_user_id,
                subject_username,
                subject_role,
                created_at,
            },
        )
        .collect();
    let next_cursor = if has_more {
        events.last().map(|event| event.id)
    } else {
        None
    };

    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((
        response_headers,
        Json(AuditPage {
            events,
            next_cursor,
        }),
    ))
}

fn parse_allowed_origins(raw: &str) -> Result<Vec<HeaderValue>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            if origin == "*" || origin == "null" {
                return Err(
                    "CORS_ALLOWED_ORIGINS must contain exact origins; '*' and 'null' are not allowed"
                        .to_owned(),
                );
            }

            origin.parse::<HeaderValue>().map_err(|_| {
                format!("CORS_ALLOWED_ORIGINS contains an invalid origin: {origin}")
            })
        })
        .collect()
}

fn cookie_secure_from_env() -> bool {
    match env::var("COOKIE_SECURE") {
        Ok(value) if value.eq_ignore_ascii_case("true") => true,
        Ok(value) if value.eq_ignore_ascii_case("false") => false,
        Ok(_) => panic!("COOKIE_SECURE must be either true or false"),
        Err(_) => true,
    }
}

fn cors_layer() -> CorsLayer {
    let configured_origins = env::var("CORS_ALLOWED_ORIGINS").unwrap_or_default();
    let origins =
        parse_allowed_origins(&configured_origins).unwrap_or_else(|error| panic!("{error}"));

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE]);

    if origins.is_empty() {
        cors
    } else {
        cors.allow_origin(AllowOrigin::list(origins))
    }
}

#[tokio::main]
async fn main() {
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL environment variable is required");

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("failed to connect to PostgreSQL");

    MIGRATOR
        .run(&db)
        .await
        .expect("failed to apply database migrations");

    match env::args().nth(1).as_deref() {
        Some("bootstrap-owner") => {
            let username = env::var("AURELIA_ADMIN_USERNAME")
                .expect("AURELIA_ADMIN_USERNAME is required for bootstrap-owner");
            let password = env::var("AURELIA_ADMIN_PASSWORD")
                .expect("AURELIA_ADMIN_PASSWORD is required for bootstrap-owner");
            auth::bootstrap_owner(&db, &username, &password)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
            println!("Initial AURELIA owner account created.");
            return;
        }
        Some("create-staff") => {
            let username = env::var("AURELIA_STAFF_USERNAME")
                .expect("AURELIA_STAFF_USERNAME is required for create-staff");
            let password = env::var("AURELIA_STAFF_PASSWORD")
                .expect("AURELIA_STAFF_PASSWORD is required for create-staff");
            let role = env::var("AURELIA_STAFF_ROLE")
                .expect("AURELIA_STAFF_ROLE is required for create-staff");
            auth::create_staff(&db, &username, &password, &role)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
            println!("AURELIA staff account created.");
            return;
        }
        Some(command) => panic!("unknown command: {command}"),
        None => {}
    }

    let csrf_allowed_origins = env::var("CSRF_ALLOWED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(str::to_owned)
        .collect();
    let state = AppState {
        db,
        cookie_secure: cookie_secure_from_env(),
        csrf_allowed_origins,
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .route("/api/menu/categories", get(list_categories))
        .route("/api/menu/items", get(list_menu_items))
        .route("/api/orders", axum::routing::post(create_order))
        .route("/api/admin/orders", get(list_admin_orders))
        .route("/api/admin/audit", get(list_audit_events))
        .route("/api/kitchen/orders", get(list_kitchen_orders))
        .route("/api/service/orders", get(list_service_orders))
        .merge(auth::routes())
        .with_state(state)
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http());

    let address: SocketAddr = "0.0.0.0:8080".parse().expect("valid bind address");
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("failed to bind API listener");
    axum::serve(listener, app).await.expect("API server failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn test_database() -> PgPool {
        let database_url = env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to a dedicated disposable test database");
        let db = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("connect to test PostgreSQL database");

        MIGRATOR
            .run(&db)
            .await
            .expect("apply SQLx migrations to test database");

        sqlx::query(
            "TRUNCATE TABLE staff_audit_log, staff_sessions, staff_users, order_lines, orders, menu_items, menu_categories RESTART IDENTITY CASCADE",
        )
        .execute(&db)
        .await
        .expect("reset dedicated test database");

        db
    }

    fn test_origin_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:8088"),
        );
        headers
    }

    async fn submit(
        db: &PgPool,
        customer_name: &str,
        items: Vec<OrderItemInput>,
    ) -> Result<(StatusCode, Json<CreatedOrder>), ApiError> {
        create_order(
            State(AppState {
                db: db.clone(),
                cookie_secure: false,
                csrf_allowed_origins: vec!["http://localhost:8088".to_owned()],
            }),
            Json(CreateOrderRequest {
                customer_name: customer_name.to_owned(),
                items,
            }),
        )
        .await
    }

    #[tokio::test]
    async fn bootstrap_owner_login_session_and_logout_work() {
        let _guard = TEST_DB_LOCK.lock().await;
        let db = test_database().await;
        let password = "a-strong-owner-password";

        auth::bootstrap_owner(&db, "Owner_01", password)
            .await
            .expect("bootstrap should create the first owner");
        let owner_id: i64 =
            sqlx::query_scalar("SELECT id FROM staff_users WHERE username = 'owner_01'")
                .fetch_one(&db)
                .await
                .expect("initial owner account should exist");

        assert!(auth::bootstrap_owner(&db, "second-owner", password)
            .await
            .is_err());

        auth::create_staff(&db, "Chef_01", "a-strong-chef-password", "chef")
            .await
            .expect("an active owner should be able to provision staff");
        assert!(auth::create_staff(&db, "bad-role", password, "owner")
            .await
            .is_err());
        let chef: (i64, String) =
            sqlx::query_as("SELECT id, role FROM staff_users WHERE username = 'chef_01'")
                .fetch_one(&db)
                .await
                .expect("provisioned chef account should exist");
        let chef_id = chef.0;
        assert_eq!(chef.1, "chef");

        let state = AppState {
            db: db.clone(),
            cookie_secure: false,
            csrf_allowed_origins: vec!["http://localhost:8088".to_owned()],
        };
        let missing_origin_login = auth::login(
            State(state.clone()),
            HeaderMap::new(),
            Json(auth::LoginRequest {
                username: "owner_01".to_owned(),
                password: password.to_owned(),
            }),
        )
        .await;
        assert!(matches!(missing_origin_login, Err(ApiError::Forbidden)));

        let bad_login = auth::login(
            State(state.clone()),
            test_origin_headers(),
            Json(auth::LoginRequest {
                username: "owner_01".to_owned(),
                password: "wrong-password".to_owned(),
            }),
        )
        .await;
        assert!(matches!(bad_login, Err(ApiError::Unauthorized)));

        let unknown_login = auth::login(
            State(state.clone()),
            test_origin_headers(),
            Json(auth::LoginRequest {
                username: "ghost_01".to_owned(),
                password: "wrong-password".to_owned(),
            }),
        )
        .await;
        assert!(matches!(unknown_login, Err(ApiError::Unauthorized)));

        let malformed_username_login = auth::login(
            State(state.clone()),
            test_origin_headers(),
            Json(auth::LoginRequest {
                username: "invalid username".to_owned(),
                password: "wrong-password".to_owned(),
            }),
        )
        .await;
        assert!(matches!(
            malformed_username_login,
            Err(ApiError::Unauthorized)
        ));

        let oversized_password_login = auth::login(
            State(state.clone()),
            test_origin_headers(),
            Json(auth::LoginRequest {
                username: "owner_01".to_owned(),
                password: "x".repeat(129),
            }),
        )
        .await;
        assert!(matches!(
            oversized_password_login,
            Err(ApiError::Unauthorized)
        ));

        let sessions_after_bad_login: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM staff_sessions")
                .fetch_one(&db)
                .await
                .expect("count sessions after failed login");
        assert_eq!(sessions_after_bad_login, 0);

        let failed_attempts: Vec<(Option<String>, Option<String>, Option<i64>)> = sqlx::query_as(
            "SELECT attempted_username, subject_role, subject_staff_user_id
             FROM staff_audit_log
             WHERE event_type = 'staff_login_failed'
             ORDER BY id",
        )
        .fetch_all(&db)
        .await
        .expect("failed login attempts should be recorded");
        assert_eq!(
            failed_attempts,
            vec![
                (Some("owner_01".to_owned()), None, None),
                (Some("ghost_01".to_owned()), None, None),
                (None, None, None),
                (Some("owner_01".to_owned()), None, None),
            ],
            "failed logins must record only a normalized username when valid, never a password or account association",
        );

        let response = auth::login(
            State(state.clone()),
            test_origin_headers(),
            Json(auth::LoginRequest {
                username: "OWNER_01".to_owned(),
                password: password.to_owned(),
            }),
        )
        .await
        .expect("valid owner credentials should create a session");

        assert_eq!(response.status(), StatusCode::OK);
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .expect("login should set a session cookie")
            .to_str()
            .expect("cookie header should be valid")
            .to_owned();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
        assert!(!set_cookie.contains("; Secure"));

        let cookie_pair = set_cookie.split(';').next().expect("cookie pair");
        let raw_token = cookie_pair
            .strip_prefix("aurelia_session=")
            .expect("session cookie name");
        let stored_hash: Vec<u8> =
            sqlx::query_scalar("SELECT token_hash FROM staff_sessions LIMIT 1")
                .fetch_one(&db)
                .await
                .expect("session token hash should be stored");
        assert_eq!(stored_hash.len(), 32);
        assert_ne!(stored_hash.as_slice(), raw_token.as_bytes());

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_bytes(cookie_pair.as_bytes()).expect("valid cookie pair"),
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:8088"),
        );

        let identity = auth::current_staff(&state, &headers)
            .await
            .expect("session should authenticate the owner");
        assert_eq!(identity.username, "owner_01");
        assert_eq!(identity.role, "owner");

        sqlx::query(
            "UPDATE staff_sessions
             SET expires_at = now() - interval '1 second'
             WHERE token_hash = $1",
        )
        .bind(stored_hash)
        .execute(&db)
        .await
        .expect("expire the test session");
        assert!(matches!(
            auth::current_staff(&state, &headers).await,
            Err(ApiError::Unauthorized)
        ));

        let logout = auth::logout(State(state.clone()), headers.clone())
            .await
            .expect("logout should revoke the session");
        assert_eq!(logout.status(), StatusCode::NO_CONTENT);
        assert!(matches!(
            auth::current_staff(&state, &headers).await,
            Err(ApiError::Unauthorized)
        ));

        let audit_events: Vec<(
            String,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT event_type, actor_staff_user_id, subject_staff_user_id,
                    subject_role, attempted_username
             FROM staff_audit_log
             ORDER BY id",
        )
        .fetch_all(&db)
        .await
        .expect("staff lifecycle audit events should be persisted");

        assert_eq!(
            audit_events
                .iter()
                .map(|event| event.0.as_str())
                .collect::<Vec<_>>(),
            vec![
                "owner_bootstrapped",
                "staff_provisioned_cli",
                "staff_login_failed",
                "staff_login_failed",
                "staff_login_failed",
                "staff_login_failed",
                "staff_login_succeeded",
                "staff_logout",
            ],
        );
        assert_eq!(audit_events[0].1, None);
        assert_eq!(audit_events[0].2, Some(owner_id));
        assert_eq!(audit_events[0].3.as_deref(), Some("owner"));
        assert_eq!(audit_events[1].1, None);
        assert_eq!(audit_events[1].2, Some(chef_id));
        assert_eq!(audit_events[1].3.as_deref(), Some("chef"));
        for failed in &audit_events[2..6] {
            assert_eq!(failed.1, None);
            assert_eq!(failed.2, None);
            assert_eq!(failed.3, None);
        }
        assert_eq!(audit_events[2].4.as_deref(), Some("owner_01"));
        assert_eq!(audit_events[3].4.as_deref(), Some("ghost_01"));
        assert_eq!(audit_events[4].4, None);
        assert_eq!(audit_events[5].4.as_deref(), Some("owner_01"));
        assert_eq!(audit_events[6].1, Some(owner_id));
        assert_eq!(audit_events[6].2, Some(owner_id));
        assert_eq!(audit_events[6].3.as_deref(), Some("owner"));
        assert_eq!(audit_events[6].4, None);
        assert_eq!(audit_events[7].1, Some(owner_id));
        assert_eq!(audit_events[7].2, Some(owner_id));
        assert_eq!(audit_events[7].3.as_deref(), Some("owner"));
        assert_eq!(audit_events[7].4, None);
    }

    #[tokio::test]
    async fn audit_endpoint_is_owner_only_and_cursor_paginated() {
        let _guard = TEST_DB_LOCK.lock().await;
        let db = test_database().await;
        let owner_password = "a-strong-owner-password";
        let chef_password = "a-strong-chef-password";

        auth::bootstrap_owner(&db, "owner_01", owner_password)
            .await
            .expect("create owner for audit endpoint test");
        auth::create_staff(&db, "chef_01", chef_password, "chef")
            .await
            .expect("create chef for audit endpoint test");

        let state = AppState {
            db: db.clone(),
            cookie_secure: false,
            csrf_allowed_origins: vec!["http://localhost:8088".to_owned()],
        };
        let query = Query(AuditQuery {
            limit: Some(2),
            before_id: None,
        });
        assert!(matches!(
            list_audit_events(State(state.clone()), HeaderMap::new(), query).await,
            Err(ApiError::Unauthorized)
        ));

        let chef_login = auth::login(
            State(state.clone()),
            test_origin_headers(),
            Json(auth::LoginRequest {
                username: "chef_01".to_owned(),
                password: chef_password.to_owned(),
            }),
        )
        .await
        .expect("chef login should succeed");
        let chef_cookie = chef_login
            .headers()
            .get(header::SET_COOKIE)
            .expect("chef login cookie")
            .to_str()
            .expect("cookie header string")
            .split(';')
            .next()
            .expect("cookie pair");
        let mut chef_headers = test_origin_headers();
        chef_headers.insert(
            header::COOKIE,
            HeaderValue::from_bytes(chef_cookie.as_bytes()).expect("valid chef cookie"),
        );
        assert!(matches!(
            list_audit_events(
                State(state.clone()),
                chef_headers,
                Query(AuditQuery {
                    limit: Some(2),
                    before_id: None,
                }),
            )
            .await,
            Err(ApiError::Forbidden)
        ));

        let owner_login = auth::login(
            State(state.clone()),
            test_origin_headers(),
            Json(auth::LoginRequest {
                username: "owner_01".to_owned(),
                password: owner_password.to_owned(),
            }),
        )
        .await
        .expect("owner login should succeed");
        let owner_cookie = owner_login
            .headers()
            .get(header::SET_COOKIE)
            .expect("owner login cookie")
            .to_str()
            .expect("cookie header string")
            .split(';')
            .next()
            .expect("cookie pair");
        let mut owner_headers = test_origin_headers();
        owner_headers.insert(
            header::COOKIE,
            HeaderValue::from_bytes(owner_cookie.as_bytes()).expect("valid owner cookie"),
        );

        let (headers, Json(first_page)) = list_audit_events(
            State(state.clone()),
            owner_headers.clone(),
            Query(AuditQuery {
                limit: Some(2),
                before_id: None,
            }),
        )
        .await
        .expect("owner should read audit log");
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(first_page.events.len(), 2);
        assert_eq!(first_page.events[0].event_type, "staff_login_succeeded");
        assert_eq!(
            first_page.events[0].actor_username.as_deref(),
            Some("owner_01")
        );
        assert_eq!(first_page.events[1].event_type, "staff_login_succeeded");
        assert_eq!(
            first_page.events[1].actor_username.as_deref(),
            Some("chef_01")
        );
        let cursor = first_page.next_cursor.expect("second page should exist");

        let (_, Json(second_page)) = list_audit_events(
            State(state.clone()),
            owner_headers.clone(),
            Query(AuditQuery {
                limit: Some(2),
                before_id: Some(cursor),
            }),
        )
        .await
        .expect("owner should read next audit page");
        assert_eq!(second_page.events.len(), 2);
        assert_eq!(second_page.events[0].event_type, "staff_provisioned_cli");
        assert_eq!(second_page.events[1].event_type, "owner_bootstrapped");
        assert_eq!(second_page.next_cursor, None);

        assert!(matches!(
            list_audit_events(
                State(state),
                owner_headers,
                Query(AuditQuery {
                    limit: Some(0),
                    before_id: None,
                }),
            )
            .await,
            Err(ApiError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn order_creation_uses_database_prices_and_rejects_invalid_orders() {
        let _guard = TEST_DB_LOCK.lock().await;
        let db = test_database().await;

        let available_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (name, price_minor, available)
             VALUES ('Integration test meal', 12500, TRUE)
             RETURNING id",
        )
        .fetch_one(&db)
        .await
        .expect("insert available test menu item");

        let (status, Json(created)) = submit(
            &db,
            "Integration Test",
            vec![OrderItemInput {
                menu_item_id: available_item_id,
                quantity: 2,
            }],
        )
        .await
        .expect("valid order should be created");

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created.total_minor, 25_000);
        assert_eq!(created.status, "pending");

        let saved_line: (i32, i64) = sqlx::query_as(
            "SELECT quantity, unit_price_minor FROM order_lines WHERE order_id = $1",
        )
        .bind(created.id)
        .fetch_one(&db)
        .await
        .expect("persisted order line should exist");
        assert_eq!(saved_line, (2, 12_500));

        let unavailable_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (name, price_minor, available)
             VALUES ('Unavailable test meal', 9000, FALSE)
             RETURNING id",
        )
        .fetch_one(&db)
        .await
        .expect("insert unavailable test menu item");

        assert!(matches!(
            submit(
                &db,
                "Integration Test",
                vec![OrderItemInput {
                    menu_item_id: unavailable_item_id,
                    quantity: 1,
                }],
            )
            .await,
            Err(ApiError::Conflict(_))
        ));

        assert!(matches!(
            submit(
                &db,
                "Integration Test",
                vec![OrderItemInput {
                    menu_item_id: available_item_id,
                    quantity: 0,
                }],
            )
            .await,
            Err(ApiError::BadRequest(_))
        ));

        assert!(matches!(
            submit(
                &db,
                "Integration Test",
                vec![OrderItemInput {
                    menu_item_id: i64::MAX,
                    quantity: 1,
                }],
            )
            .await,
            Err(ApiError::NotFound(_))
        ));

        let overflow_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (name, price_minor, available)
             VALUES ('Overflow test meal', $1, TRUE)
             RETURNING id",
        )
        .bind(i64::MAX)
        .fetch_one(&db)
        .await
        .expect("insert overflow test menu item");

        assert!(matches!(
            submit(
                &db,
                "Integration Test",
                vec![OrderItemInput {
                    menu_item_id: overflow_item_id,
                    quantity: 2,
                }],
            )
            .await,
            Err(ApiError::BadRequest(_))
        ));

        let order_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders")
            .fetch_one(&db)
            .await
            .expect("count persisted orders");
        assert_eq!(order_count, 1, "rejected orders must not be persisted");
    }
}
