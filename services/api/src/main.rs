mod auth;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
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
struct RestaurantSummary {
    id: i64,
    slug: String,
    name: String,
    city: String,
    cuisine: String,
    description: String,
}

#[derive(Deserialize)]
struct RestaurantApplicationRequest {
    restaurant_name: String,
    owner_username: String,
    password: String,
    contact_name: String,
    contact_phone: String,
    contact_email: String,
    city: String,
    address: String,
    cuisine: String,
    description: String,
}

#[derive(Serialize)]
struct RestaurantApplicationSubmitted {
    application_id: i64,
    restaurant_id: i64,
    status: String,
}

#[derive(Serialize)]
struct PendingRestaurantApplication {
    id: i64,
    restaurant_id: i64,
    restaurant_name: String,
    owner_username: String,
    contact_name: String,
    contact_phone: String,
    contact_email: String,
    city: String,
    address: String,
    cuisine: String,
    description: String,
    submitted_at: String,
}

#[derive(Deserialize)]
struct ReviewRestaurantApplicationRequest {
    decision: String,
    note: Option<String>,
}

#[derive(Serialize)]
struct RestaurantApplicationReview {
    application_id: i64,
    restaurant_id: i64,
    status: String,
}

#[derive(Deserialize)]
struct RestaurantFilter {
    restaurant_id: Option<i64>,
}

#[derive(Serialize)]
struct MenuCategory {
    id: i64,
    restaurant_id: i64,
    restaurant_name: String,
    name: String,
    slug: String,
}

#[derive(Serialize)]
struct MenuItem {
    id: i64,
    restaurant_id: i64,
    restaurant_name: String,
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
struct UpdateOrderStatusRequest {
    status: String,
}

#[derive(Serialize)]
struct OrderStatusUpdate {
    id: i64,
    order_number: String,
    status: String,
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

fn valid_contact_phone(phone: &str) -> bool {
    let digits = phone
        .chars()
        .filter(|character| character.is_ascii_digit())
        .count();
    (8..=15).contains(&digits)
        && phone.chars().all(|character| {
            character.is_ascii_digit() || matches!(character, '+' | ' ' | '-' | '(' | ')')
        })
}

async fn submit_restaurant_application(
    State(state): State<AppState>,
    Json(payload): Json<RestaurantApplicationRequest>,
) -> Result<(StatusCode, Json<RestaurantApplicationSubmitted>), ApiError> {
    let restaurant_name = payload.restaurant_name.trim();
    let contact_name = payload.contact_name.trim();
    let contact_phone = payload.contact_phone.trim();
    let contact_email = payload.contact_email.trim().to_ascii_lowercase();
    let city = payload.city.trim();
    let address = payload.address.trim();
    let cuisine = payload.cuisine.trim();
    let description = payload.description.trim();

    if !(1..=160).contains(&restaurant_name.chars().count())
        || !(1..=120).contains(&contact_name.chars().count())
        || !valid_contact_phone(contact_phone)
        || !(1..=80).contains(&city.chars().count())
        || !(5..=500).contains(&address.chars().count())
        || !(1..=100).contains(&cuisine.chars().count())
        || description.chars().count() > 1000
        || contact_email.len() > 254
        || !contact_email.contains('@')
        || contact_email.split('@').count() != 2
    {
        return Err(ApiError::BadRequest(
            "restaurant application fields are invalid",
        ));
    }

    let owner_username = auth::normalize_username(&payload.owner_username)
        .map_err(|_| ApiError::BadRequest("owner username is invalid"))?;
    auth::validate_password(&payload.password)
        .map_err(|_| ApiError::BadRequest("password must contain 12-128 bytes"))?;
    let password_hash = auth::hash_password(&payload.password).map_err(|_| ApiError::Internal)?;

    let mut tx = state.db.begin().await.map_err(|_| ApiError::Internal)?;
    let username_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM staff_users WHERE username = $1)")
            .bind(&owner_username)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| ApiError::Internal)?;
    if username_exists {
        return Err(ApiError::Conflict("username is already taken"));
    }

    let slug = format!("restaurant-{}", Uuid::new_v4().simple());
    let restaurant_id: i64 = sqlx::query_scalar(
        "INSERT INTO restaurants
            (slug, name, status, description, contact_phone, contact_email,
             city, address, cuisine, delivery_mode)
         VALUES ($1, $2, 'pending_review', $3, $4, $5, $6, $7, $8, 'restaurant')
         RETURNING id",
    )
    .bind(slug)
    .bind(restaurant_name)
    .bind(description)
    .bind(contact_phone)
    .bind(&contact_email)
    .bind(city)
    .bind(address)
    .bind(cuisine)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    let owner_staff_user_id: i64 = sqlx::query_scalar(
        "INSERT INTO staff_users (username, password_hash, role, is_active)
         VALUES ($1, $2, 'manager', FALSE)
         RETURNING id",
    )
    .bind(&owner_username)
    .bind(password_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError::Conflict("username is already taken"))?;

    sqlx::query(
        "INSERT INTO restaurant_staff_memberships (restaurant_id, staff_user_id, role, is_active)
         VALUES ($1, $2, 'restaurant_owner', FALSE)",
    )
    .bind(restaurant_id)
    .bind(owner_staff_user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    let application_id: i64 = sqlx::query_scalar(
        "INSERT INTO restaurant_applications
            (restaurant_id, owner_staff_user_id, contact_name)
         VALUES ($1, $2, $3)
         RETURNING id",
    )
    .bind(restaurant_id)
    .bind(owner_staff_user_id)
    .bind(contact_name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    sqlx::query(
        "INSERT INTO staff_audit_log
            (event_type, actor_staff_user_id, subject_staff_user_id, subject_role, restaurant_id)
         VALUES ('restaurant_application_submitted', $1, $1, 'restaurant_owner', $2)",
    )
    .bind(owner_staff_user_id)
    .bind(restaurant_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    tx.commit().await.map_err(|_| ApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(RestaurantApplicationSubmitted {
            application_id,
            restaurant_id,
            status: "pending_review".to_owned(),
        }),
    ))
}

async fn list_pending_restaurant_applications(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<PendingRestaurantApplication>>, ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if staff.role != "owner" {
        return Err(ApiError::Forbidden);
    }

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ),
    >(
        "SELECT a.id, r.id, r.name, u.username, a.contact_name, r.contact_phone,
                r.contact_email, r.city, r.address, r.cuisine, r.description,
                to_char(a.submitted_at, 'YYYY-MM-DD HH24:MI:SS')
         FROM restaurant_applications a
         INNER JOIN restaurants r ON r.id = a.restaurant_id
         INNER JOIN staff_users u ON u.id = a.owner_staff_user_id
         WHERE a.status = 'pending_review'
         ORDER BY a.submitted_at ASC
         LIMIT 200",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    restaurant_id,
                    restaurant_name,
                    owner_username,
                    contact_name,
                    contact_phone,
                    contact_email,
                    city,
                    address,
                    cuisine,
                    description,
                    submitted_at,
                )| PendingRestaurantApplication {
                    id,
                    restaurant_id,
                    restaurant_name,
                    owner_username,
                    contact_name,
                    contact_phone,
                    contact_email,
                    city,
                    address,
                    cuisine,
                    description,
                    submitted_at,
                },
            )
            .collect(),
    ))
}

async fn review_restaurant_application(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(application_id): Path<i64>,
    Json(payload): Json<ReviewRestaurantApplicationRequest>,
) -> Result<(StatusCode, Json<RestaurantApplicationReview>), ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if staff.role != "owner" {
        return Err(ApiError::Forbidden);
    }
    if application_id <= 0 || !matches!(payload.decision.as_str(), "approve" | "reject") {
        return Err(ApiError::BadRequest(
            "application id or decision is invalid",
        ));
    }
    let note = payload.note.unwrap_or_default().trim().to_owned();
    if note.chars().count() > 1000 {
        return Err(ApiError::BadRequest("review note is too long"));
    }

    let approve = payload.decision == "approve";
    let application_status = if approve { "approved" } else { "rejected" };
    let restaurant_status = if approve { "active" } else { "rejected" };
    let event_type = if approve {
        "restaurant_application_approved"
    } else {
        "restaurant_application_rejected"
    };

    let mut tx = state.db.begin().await.map_err(|_| ApiError::Internal)?;
    let application: Option<(i64, i64, String)> = sqlx::query_as(
        "SELECT restaurant_id, owner_staff_user_id, status
         FROM restaurant_applications WHERE id = $1 FOR UPDATE",
    )
    .bind(application_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;
    let (restaurant_id, owner_staff_user_id, current_status) =
        application.ok_or(ApiError::NotFound("application not found"))?;
    if current_status != "pending_review" {
        return Err(ApiError::Conflict("application has already been reviewed"));
    }

    sqlx::query(
        "UPDATE restaurant_applications
         SET status = $1, reviewed_by_staff_user_id = $2, reviewed_at = now(), review_note = $3
         WHERE id = $4",
    )
    .bind(application_status)
    .bind(staff.id)
    .bind(note)
    .bind(application_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    sqlx::query("UPDATE restaurants SET status = $1 WHERE id = $2")
        .bind(restaurant_status)
        .bind(restaurant_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::Internal)?;

    sqlx::query("UPDATE staff_users SET is_active = $1 WHERE id = $2")
        .bind(approve)
        .bind(owner_staff_user_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::Internal)?;

    sqlx::query(
        "UPDATE restaurant_staff_memberships SET is_active = $1
         WHERE restaurant_id = $2 AND staff_user_id = $3 AND role = 'restaurant_owner'",
    )
    .bind(approve)
    .bind(restaurant_id)
    .bind(owner_staff_user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    sqlx::query(
        "INSERT INTO staff_audit_log
            (event_type, actor_staff_user_id, subject_staff_user_id, subject_role, restaurant_id)
         VALUES ($1, $2, $3, 'restaurant_owner', $4)",
    )
    .bind(event_type)
    .bind(staff.id)
    .bind(owner_staff_user_id)
    .bind(restaurant_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    tx.commit().await.map_err(|_| ApiError::Internal)?;

    Ok((
        StatusCode::OK,
        Json(RestaurantApplicationReview {
            application_id,
            restaurant_id,
            status: application_status.to_owned(),
        }),
    ))
}

async fn list_restaurants(
    State(state): State<AppState>,
) -> Result<Json<Vec<RestaurantSummary>>, ApiError> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, String, String)>(
        "SELECT id, slug, name, city, cuisine, description
         FROM restaurants
         WHERE status = 'active'
         ORDER BY name ASC
         LIMIT 200",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(id, slug, name, city, cuisine, description)| RestaurantSummary {
                    id,
                    slug,
                    name,
                    city,
                    cuisine,
                    description,
                },
            )
            .collect(),
    ))
}

async fn list_categories(
    State(state): State<AppState>,
    Query(filter): Query<RestaurantFilter>,
) -> Result<Json<Vec<MenuCategory>>, ApiError> {
    if filter.restaurant_id.is_some_and(|id| id <= 0) {
        return Err(ApiError::BadRequest("restaurant_id must be positive"));
    }

    let rows = sqlx::query_as::<_, (i64, i64, String, String, String)>(
        "SELECT mc.id, mc.restaurant_id, r.name, mc.name, mc.slug
         FROM menu_categories mc
         INNER JOIN restaurants r ON r.id = mc.restaurant_id
         WHERE r.status = 'active'
           AND ($1::bigint IS NULL OR mc.restaurant_id = $1)
         ORDER BY r.name ASC, mc.name ASC
         LIMIT 500",
    )
    .bind(filter.restaurant_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(id, restaurant_id, restaurant_name, name, slug)| MenuCategory {
                    id,
                    restaurant_id,
                    restaurant_name,
                    name,
                    slug,
                },
            )
            .collect(),
    ))
}

async fn list_menu_items(
    State(state): State<AppState>,
    Query(filter): Query<RestaurantFilter>,
) -> Result<Json<Vec<MenuItem>>, ApiError> {
    if filter.restaurant_id.is_some_and(|id| id <= 0) {
        return Err(ApiError::BadRequest("restaurant_id must be positive"));
    }

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            Option<i64>,
            Option<String>,
            String,
            String,
            i64,
            bool,
        ),
    >(
        "SELECT mi.id, mi.restaurant_id, r.name, mi.category_id, mc.name,
                mi.name, mi.description, mi.price_minor, mi.available
         FROM menu_items mi
         INNER JOIN restaurants r ON r.id = mi.restaurant_id
         LEFT JOIN menu_categories mc
           ON mc.restaurant_id = mi.restaurant_id AND mc.id = mi.category_id
         WHERE r.status = 'active'
           AND ($1::bigint IS NULL OR mi.restaurant_id = $1)
         ORDER BY r.name ASC, mi.name ASC
         LIMIT 1000",
    )
    .bind(filter.restaurant_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    restaurant_id,
                    restaurant_name,
                    category_id,
                    category_name,
                    name,
                    description,
                    price_minor,
                    available,
                )| MenuItem {
                    id,
                    restaurant_id,
                    restaurant_name,
                    category_id,
                    category_name,
                    name,
                    description,
                    price_minor,
                    available,
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
    let mut restaurant_id: Option<i64> = None;
    let mut order_lines = Vec::with_capacity(payload.items.len());

    for item in &payload.items {
        let menu_item = sqlx::query_as::<_, (i64, bool, i64, String)>(
            "SELECT mi.price_minor, mi.available, mi.restaurant_id, r.status
             FROM menu_items mi
             INNER JOIN restaurants r ON r.id = mi.restaurant_id
             WHERE mi.id = $1 FOR SHARE OF mi",
        )
        .bind(item.menu_item_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound("one or more menu items do not exist"))?;

        let (unit_price_minor, available, item_restaurant_id, restaurant_status) = menu_item;
        if restaurant_status != "active" {
            return Err(ApiError::Conflict("restaurant is not accepting orders"));
        }
        if !available {
            return Err(ApiError::Conflict("one or more menu items are unavailable"));
        }
        if restaurant_id.is_some_and(|id| id != item_restaurant_id) {
            return Err(ApiError::BadRequest(
                "orders may contain items from one restaurant only",
            ));
        }
        restaurant_id = Some(item_restaurant_id);

        let line_total = unit_price_minor
            .checked_mul(i64::from(item.quantity))
            .ok_or(ApiError::BadRequest("order total is out of range"))?;
        total_minor = total_minor
            .checked_add(line_total)
            .ok_or(ApiError::BadRequest("order total is out of range"))?;
        order_lines.push((
            item_restaurant_id,
            item.menu_item_id,
            item.quantity,
            unit_price_minor,
        ));
    }

    let restaurant_id = restaurant_id.ok_or(ApiError::BadRequest("items are required"))?;
    let order_number = format!("AUR-{}", Uuid::new_v4().simple());
    let (order_id, status) = sqlx::query_as::<_, (i64, String)>(
        "INSERT INTO orders (restaurant_id, order_number, customer_name, status, total_minor)
         VALUES ($1, $2, $3, 'pending', $4)
         RETURNING id, status",
    )
    .bind(restaurant_id)
    .bind(&order_number)
    .bind(customer_name)
    .bind(total_minor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    for (line_restaurant_id, menu_item_id, quantity, unit_price_minor) in order_lines {
        sqlx::query(
            "INSERT INTO order_lines
                (restaurant_id, order_id, menu_item_id, quantity, unit_price_minor)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(line_restaurant_id)
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

async fn fetch_orders(
    db: &PgPool,
    active_only: bool,
    restaurant_id: Option<i64>,
) -> Result<Response, ApiError> {
    let rows = if active_only {
        sqlx::query_as::<_, (i64, String, String, String, i64)>(
            "SELECT id, order_number, customer_name, status, total_minor
             FROM orders
             WHERE ($1::bigint IS NULL OR restaurant_id = $1)
               AND status IN ('pending', 'accepted', 'preparing', 'ready')
             ORDER BY created_at ASC
             LIMIT 200",
        )
        .bind(restaurant_id)
        .fetch_all(db)
        .await
        .map_err(|_| ApiError::Internal)?
    } else {
        sqlx::query_as::<_, (i64, String, String, String, i64)>(
            "SELECT id, order_number, customer_name, status, total_minor
             FROM orders
             WHERE ($1::bigint IS NULL OR restaurant_id = $1)
             ORDER BY created_at DESC
             LIMIT 200",
        )
        .bind(restaurant_id)
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

fn valid_order_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("pending", "accepted")
            | ("accepted", "preparing")
            | ("preparing", "ready")
            | ("ready", "completed")
            | ("pending", "cancelled")
            | ("accepted", "cancelled")
            | ("preparing", "cancelled")
    )
}

fn role_can_update_order_status(role: &str, from: &str, to: &str) -> bool {
    if matches!(role, "owner" | "restaurant_owner" | "manager") {
        return valid_order_transition(from, to);
    }

    match role {
        "chef" => matches!(
            (from, to),
            ("pending", "accepted") | ("accepted", "preparing") | ("preparing", "ready")
        ),
        "waiter" => matches!(
            (from, to),
            ("ready", "completed") | ("pending", "cancelled")
        ),
        "cashier" => matches!(
            (from, to),
            ("pending", "cancelled") | ("accepted", "cancelled")
        ),
        _ => false,
    }
}

async fn update_order_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(order_id): Path<i64>,
    Json(payload): Json<UpdateOrderStatusRequest>,
) -> Result<(StatusCode, HeaderMap, Json<OrderStatusUpdate>), ApiError> {
    auth::validate_request_origin(&state, &headers)?;
    let staff = auth::current_staff(&state, &headers).await?;

    if order_id <= 0
        || !matches!(
            payload.status.as_str(),
            "pending" | "accepted" | "preparing" | "ready" | "completed" | "cancelled"
        )
    {
        return Err(ApiError::BadRequest("order_id and status must be valid"));
    }

    let mut tx = state.db.begin().await.map_err(|_| ApiError::Internal)?;
    let order: Option<(String, i64)> =
        sqlx::query_as("SELECT status, restaurant_id FROM orders WHERE id = $1 FOR UPDATE")
            .bind(order_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ApiError::Internal)?;
    let (current_status, restaurant_id) = order.ok_or(ApiError::NotFound("order not found"))?;

    let restaurant_role = if staff.role == "owner" {
        "owner".to_owned()
    } else {
        sqlx::query_scalar::<_, String>(
            "SELECT role FROM restaurant_staff_memberships
             WHERE staff_user_id = $1 AND restaurant_id = $2 AND is_active = TRUE",
        )
        .bind(staff.id)
        .bind(restaurant_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?
    };

    if !valid_order_transition(&current_status, &payload.status) {
        return Err(ApiError::Conflict("invalid order status transition"));
    }
    if !role_can_update_order_status(&restaurant_role, &current_status, &payload.status) {
        return Err(ApiError::Forbidden);
    }

    let (id, order_number, status) = sqlx::query_as::<_, (i64, String, String)>(
        "UPDATE orders SET status = $1 WHERE id = $2
         RETURNING id, order_number, status",
    )
    .bind(&payload.status)
    .bind(order_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    sqlx::query(
        "INSERT INTO order_status_events
            (restaurant_id, order_id, actor_staff_user_id, from_status, to_status)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(restaurant_id)
    .bind(order_id)
    .bind(staff.id)
    .bind(&current_status)
    .bind(&status)
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError::Internal)?;

    tx.commit().await.map_err(|_| ApiError::Internal)?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((
        StatusCode::OK,
        response_headers,
        Json(OrderStatusUpdate {
            id,
            order_number,
            status,
        }),
    ))
}

async fn first_staff_restaurant_id(db: &PgPool, staff_user_id: i64) -> Result<i64, ApiError> {
    sqlx::query_scalar(
        "SELECT restaurant_id FROM restaurant_staff_memberships
         WHERE staff_user_id = $1 AND is_active = TRUE
         ORDER BY restaurant_id ASC LIMIT 1",
    )
    .bind(staff_user_id)
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError::Internal)?
    .ok_or(ApiError::Forbidden)
}

async fn restaurant_staff_role(
    db: &PgPool,
    staff_user_id: i64,
    restaurant_id: i64,
) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar(
        "SELECT role FROM restaurant_staff_memberships
         WHERE staff_user_id = $1 AND restaurant_id = $2 AND is_active = TRUE",
    )
    .bind(staff_user_id)
    .bind(restaurant_id)
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError::Internal)
}

async fn list_admin_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if staff.role == "owner" {
        return fetch_orders(&state.db, false, None).await;
    }

    let restaurant_id = first_staff_restaurant_id(&state.db, staff.id).await?;
    let role = restaurant_staff_role(&state.db, staff.id, restaurant_id)
        .await?
        .ok_or(ApiError::Forbidden)?;
    if !matches!(role.as_str(), "restaurant_owner" | "manager" | "cashier") {
        return Err(ApiError::Forbidden);
    }
    fetch_orders(&state.db, false, Some(restaurant_id)).await
}

async fn list_kitchen_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if staff.role == "owner" {
        return fetch_orders(&state.db, true, None).await;
    }

    let restaurant_id = default_restaurant_id(&state.db).await?;
    let role = restaurant_staff_role(&state.db, staff.id, restaurant_id)
        .await?
        .ok_or(ApiError::Forbidden)?;
    if !matches!(role.as_str(), "restaurant_owner" | "manager" | "chef") {
        return Err(ApiError::Forbidden);
    }
    fetch_orders(&state.db, true, Some(restaurant_id)).await
}

async fn list_service_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let staff = auth::current_staff(&state, &headers).await?;
    if staff.role == "owner" {
        return fetch_orders(&state.db, true, None).await;
    }

    let restaurant_id = default_restaurant_id(&state.db).await?;
    let role = restaurant_staff_role(&state.db, staff.id, restaurant_id)
        .await?
        .ok_or(ApiError::Forbidden)?;
    if !matches!(role.as_str(), "restaurant_owner" | "manager" | "waiter") {
        return Err(ApiError::Forbidden);
    }
    fetch_orders(&state.db, true, Some(restaurant_id)).await
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
        .route("/api/restaurants", get(list_restaurants))
        .route(
            "/api/restaurants/apply",
            post(submit_restaurant_application),
        )
        .route(
            "/api/admin/restaurants/applications",
            get(list_pending_restaurant_applications),
        )
        .route(
            "/api/admin/restaurants/applications/:application_id/review",
            post(review_restaurant_application),
        )
        .route("/api/menu/categories", get(list_categories))
        .route("/api/menu/items", get(list_menu_items))
        .route("/api/orders", axum::routing::post(create_order))
        .route("/api/orders/:order_id/status", post(update_order_status))
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
            "TRUNCATE TABLE staff_audit_log, staff_sessions, staff_users,
             restaurant_applications, restaurant_staff_memberships,
             order_status_events, order_lines, orders, menu_items, menu_categories,
             restaurants RESTART IDENTITY CASCADE",
        )
        .execute(&db)
        .await
        .expect("reset dedicated test database");

        sqlx::query(
            "INSERT INTO restaurants (id, slug, name, status, is_default)
             VALUES (1, 'legacy-default', 'AURELIA Legacy Restaurant', 'active', TRUE)",
        )
        .execute(&db)
        .await
        .expect("reseed default restaurant for tests");
        sqlx::query("SELECT setval('restaurants_id_seq', 1, TRUE)")
            .execute(&db)
            .await
            .expect("reset restaurant sequence");

        db
    }

    #[tokio::test]
    async fn restaurant_application_review_activates_owner_and_tenant() {
        let _guard = TEST_DB_LOCK.lock().await;
        let db = test_database().await;
        let state = AppState {
            db: db.clone(),
            cookie_secure: false,
            csrf_allowed_origins: vec!["http://localhost:8088".to_owned()],
        };

        auth::bootstrap_owner(&db, "platform_owner", "platform-owner-password-strong")
            .await
            .expect("create platform owner");
        let owner_headers =
            staff_session_headers(&state, "platform_owner", "platform-owner-password-strong").await;

        let (submitted_status, Json(submitted)) = submit_restaurant_application(
            State(state.clone()),
            Json(RestaurantApplicationRequest {
                restaurant_name: "North Kitchen".to_owned(),
                owner_username: "north_owner".to_owned(),
                password: "north-owner-password-strong".to_owned(),
                contact_name: "Restaurant Owner".to_owned(),
                contact_phone: "+989121234567".to_owned(),
                contact_email: "owner@example.test".to_owned(),
                city: "Tehran".to_owned(),
                address: "A test street, Tehran".to_owned(),
                cuisine: "Iranian".to_owned(),
                description: "A test application".to_owned(),
            }),
        )
        .await
        .expect("valid application should submit");
        assert_eq!(submitted_status, StatusCode::CREATED);
        assert_eq!(submitted.status, "pending_review");

        let pending_owner_active: bool =
            sqlx::query_scalar("SELECT is_active FROM staff_users WHERE username = 'north_owner'")
                .fetch_one(&db)
                .await
                .expect("owner account should be created");
        assert!(
            !pending_owner_active,
            "owner login must remain disabled until review"
        );

        let pending =
            list_pending_restaurant_applications(State(state.clone()), owner_headers.clone())
                .await
                .expect("platform owner may list pending applications");
        assert_eq!(pending.0.len(), 1);
        assert_eq!(pending.0[0].restaurant_name, "North Kitchen");
        assert_eq!(pending.0[0].owner_username, "north_owner");

        let (review_status, Json(approved)) = review_restaurant_application(
            State(state.clone()),
            owner_headers.clone(),
            Path(submitted.application_id),
            Json(ReviewRestaurantApplicationRequest {
                decision: "approve".to_owned(),
                note: Some("Reviewed for test".to_owned()),
            }),
        )
        .await
        .expect("owner may approve a pending application");
        assert_eq!(review_status, StatusCode::OK);
        assert_eq!(approved.status, "approved");

        let restaurant_status: String =
            sqlx::query_scalar("SELECT status FROM restaurants WHERE id = $1")
                .bind(submitted.restaurant_id)
                .fetch_one(&db)
                .await
                .expect("restaurant status should be updated");
        assert_eq!(restaurant_status, "active");
        let membership: (String, bool) = sqlx::query_as(
            "SELECT role, is_active FROM restaurant_staff_memberships
             WHERE restaurant_id = $1 AND staff_user_id = (
                SELECT id FROM staff_users WHERE username = 'north_owner'
             )",
        )
        .bind(submitted.restaurant_id)
        .fetch_one(&db)
        .await
        .expect("owner membership should exist");
        assert_eq!(membership, ("restaurant_owner".to_owned(), true));

        let owner_session =
            staff_session_headers(&state, "north_owner", "north-owner-password-strong").await;
        let tenant_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (restaurant_id, name, price_minor, available)
             VALUES ($1, 'North Kitchen Dish', 42000, TRUE)
             RETURNING id",
        )
        .bind(submitted.restaurant_id)
        .fetch_one(&db)
        .await
        .expect("insert tenant menu item");
        let (_, Json(tenant_order)) = submit(
            &db,
            "Marketplace Test Customer",
            vec![OrderItemInput {
                menu_item_id: tenant_item_id,
                quantity: 1,
            }],
        )
        .await
        .expect("create tenant order");

        let owner_orders = list_admin_orders(State(state.clone()), owner_session)
            .await
            .expect("approved restaurant owner may read own order inbox");
        let owner_orders_body = axum::body::to_bytes(owner_orders.into_body(), usize::MAX)
            .await
            .expect("read restaurant order inbox");
        let owner_orders_json =
            String::from_utf8(owner_orders_body.to_vec()).expect("response should be UTF-8");
        assert!(owner_orders_json.contains(&tenant_order.order_number));

        let (_, Json(rejected_application)) = submit_restaurant_application(
            State(state.clone()),
            Json(RestaurantApplicationRequest {
                restaurant_name: "Rejected Kitchen".to_owned(),
                owner_username: "rejected_owner".to_owned(),
                password: "rejected-owner-password-strong".to_owned(),
                contact_name: "Rejected Owner".to_owned(),
                contact_phone: "09123456789".to_owned(),
                contact_email: "rejected@example.test".to_owned(),
                city: "Tehran".to_owned(),
                address: "Another test street".to_owned(),
                cuisine: "Fast food".to_owned(),
                description: String::new(),
            }),
        )
        .await
        .expect("second valid application should submit");
        let (_, Json(rejected)) = review_restaurant_application(
            State(state.clone()),
            owner_headers.clone(),
            Path(rejected_application.application_id),
            Json(ReviewRestaurantApplicationRequest {
                decision: "reject".to_owned(),
                note: Some("Unable to verify in test".to_owned()),
            }),
        )
        .await
        .expect("owner may reject an application");
        assert_eq!(rejected.status, "rejected");

        let rejected_state: (String, bool) = sqlx::query_as(
            "SELECT r.status, u.is_active
             FROM restaurants r
             INNER JOIN restaurant_applications a ON a.restaurant_id = r.id
             INNER JOIN staff_users u ON u.id = a.owner_staff_user_id
             WHERE a.id = $1",
        )
        .bind(rejected_application.application_id)
        .fetch_one(&db)
        .await
        .expect("read rejected application state");
        assert_eq!(rejected_state, ("rejected".to_owned(), false));

        let remaining = list_pending_restaurant_applications(State(state), owner_headers)
            .await
            .expect("read remaining pending applications");
        assert!(remaining.0.is_empty());
    }

    #[tokio::test]
    async fn restaurant_application_review_activates_owner_and_tenant() {
        let _guard = TEST_DB_LOCK.lock().await;
        let db = test_database().await;
        let state = AppState {
            db: db.clone(),
            cookie_secure: false,
            csrf_allowed_origins: vec!["http://localhost:8088".to_owned()],
        };

        auth::bootstrap_owner(&db, "platform_owner", "platform-owner-password-strong")
            .await
            .expect("create platform owner");
        let owner_headers =
            staff_session_headers(&state, "platform_owner", "platform-owner-password-strong").await;

        let (submitted_status, Json(submitted)) = submit_restaurant_application(
            State(state.clone()),
            Json(RestaurantApplicationRequest {
                restaurant_name: "North Kitchen".to_owned(),
                owner_username: "north_owner".to_owned(),
                password: "north-owner-password-strong".to_owned(),
                contact_name: "Restaurant Owner".to_owned(),
                contact_phone: "+989121234567".to_owned(),
                contact_email: "owner@example.test".to_owned(),
                city: "Tehran".to_owned(),
                address: "A test street, Tehran".to_owned(),
                cuisine: "Iranian".to_owned(),
                description: "A test application".to_owned(),
            }),
        )
        .await
        .expect("valid application should submit");
        assert_eq!(submitted_status, StatusCode::CREATED);
        assert_eq!(submitted.status, "pending_review");

        let pending_owner_active: bool =
            sqlx::query_scalar("SELECT is_active FROM staff_users WHERE username = 'north_owner'")
                .fetch_one(&db)
                .await
                .expect("owner account should be created");
        assert!(
            !pending_owner_active,
            "owner login must remain disabled until review"
        );

        let pending =
            list_pending_restaurant_applications(State(state.clone()), owner_headers.clone())
                .await
                .expect("platform owner may list pending applications");
        assert_eq!(pending.0.len(), 1);
        assert_eq!(pending.0[0].restaurant_name, "North Kitchen");
        assert_eq!(pending.0[0].owner_username, "north_owner");

        let (review_status, Json(approved)) = review_restaurant_application(
            State(state.clone()),
            owner_headers.clone(),
            Path(submitted.application_id),
            Json(ReviewRestaurantApplicationRequest {
                decision: "approve".to_owned(),
                note: Some("Reviewed for test".to_owned()),
            }),
        )
        .await
        .expect("owner may approve a pending application");
        assert_eq!(review_status, StatusCode::OK);
        assert_eq!(approved.status, "approved");

        let restaurant_status: String =
            sqlx::query_scalar("SELECT status FROM restaurants WHERE id = $1")
                .bind(submitted.restaurant_id)
                .fetch_one(&db)
                .await
                .expect("restaurant status should be updated");
        assert_eq!(restaurant_status, "active");
        let membership: (String, bool) = sqlx::query_as(
            "SELECT role, is_active FROM restaurant_staff_memberships
             WHERE restaurant_id = $1 AND staff_user_id = (
                SELECT id FROM staff_users WHERE username = 'north_owner'
             )",
        )
        .bind(submitted.restaurant_id)
        .fetch_one(&db)
        .await
        .expect("owner membership should exist");
        assert_eq!(membership, ("restaurant_owner".to_owned(), true));

        let owner_session =
            staff_session_headers(&state, "north_owner", "north-owner-password-strong").await;
        let tenant_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (restaurant_id, name, price_minor, available)
             VALUES ($1, 'North Kitchen Dish', 42000, TRUE)
             RETURNING id",
        )
        .bind(submitted.restaurant_id)
        .fetch_one(&db)
        .await
        .expect("insert tenant menu item");
        let (_, Json(tenant_order)) = submit(
            &db,
            "Marketplace Test Customer",
            vec![OrderItemInput {
                menu_item_id: tenant_item_id,
                quantity: 1,
            }],
        )
        .await
        .expect("create tenant order");

        let owner_orders = list_admin_orders(State(state.clone()), owner_session)
            .await
            .expect("approved restaurant owner may read own order inbox");
        let owner_orders_body = axum::body::to_bytes(owner_orders.into_body(), usize::MAX)
            .await
            .expect("read restaurant order inbox");
        let owner_orders_json =
            String::from_utf8(owner_orders_body.to_vec()).expect("response should be UTF-8");
        assert!(owner_orders_json.contains(&tenant_order.order_number));

        let (_, Json(rejected_application)) = submit_restaurant_application(
            State(state.clone()),
            Json(RestaurantApplicationRequest {
                restaurant_name: "Rejected Kitchen".to_owned(),
                owner_username: "rejected_owner".to_owned(),
                password: "rejected-owner-password-strong".to_owned(),
                contact_name: "Rejected Owner".to_owned(),
                contact_phone: "09123456789".to_owned(),
                contact_email: "rejected@example.test".to_owned(),
                city: "Tehran".to_owned(),
                address: "Another test street".to_owned(),
                cuisine: "Fast food".to_owned(),
                description: String::new(),
            }),
        )
        .await
        .expect("second valid application should submit");
        let (_, Json(rejected)) = review_restaurant_application(
            State(state.clone()),
            owner_headers.clone(),
            Path(rejected_application.application_id),
            Json(ReviewRestaurantApplicationRequest {
                decision: "reject".to_owned(),
                note: Some("Unable to verify in test".to_owned()),
            }),
        )
        .await
        .expect("owner may reject an application");
        assert_eq!(rejected.status, "rejected");

        let rejected_state: (String, bool) = sqlx::query_as(
            "SELECT r.status, u.is_active
             FROM restaurants r
             INNER JOIN restaurant_applications a ON a.restaurant_id = r.id
             INNER JOIN staff_users u ON u.id = a.owner_staff_user_id
             WHERE a.id = $1",
        )
        .bind(rejected_application.application_id)
        .fetch_one(&db)
        .await
        .expect("read rejected application state");
        assert_eq!(rejected_state, ("rejected".to_owned(), false));

        let remaining = list_pending_restaurant_applications(State(state), owner_headers)
            .await
            .expect("read remaining pending applications");
        assert!(remaining.0.is_empty());
    }

    #[tokio::test]
    async fn marketplace_tenant_schema_scopes_legacy_rows_and_rejects_cross_tenant_links() {
        let _guard = TEST_DB_LOCK.lock().await;
        let db = test_database().await;

        let default_restaurant: (i64, String, bool) =
            sqlx::query_as("SELECT id, slug, is_default FROM restaurants WHERE is_default = TRUE")
                .fetch_one(&db)
                .await
                .expect("migration should seed a default legacy restaurant");
        assert_eq!(default_restaurant, (1, "legacy-default".to_owned(), true));

        let other_slug = format!("tenant-test-{}", Uuid::new_v4().simple());
        let other_restaurant_id: i64 = sqlx::query_scalar(
            "INSERT INTO restaurants (slug, name, status)
             VALUES ($1, 'Tenant Test Restaurant', 'active')
             RETURNING id",
        )
        .bind(other_slug)
        .fetch_one(&db)
        .await
        .expect("create a second tenant");

        let default_category_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_categories (name, slug)
             VALUES ('Legacy Category', 'legacy-category')
             RETURNING id",
        )
        .fetch_one(&db)
        .await
        .expect("legacy category should default to tenant 1");

        let default_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (name, price_minor, available)
             VALUES ('Legacy Item', 1000, TRUE)
             RETURNING id",
        )
        .fetch_one(&db)
        .await
        .expect("legacy menu item should default to tenant 1");

        let second_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (restaurant_id, name, price_minor, available)
             VALUES ($1, 'Second Tenant Item', 2000, TRUE)
             RETURNING id",
        )
        .bind(other_restaurant_id)
        .fetch_one(&db)
        .await
        .expect("create a menu item for tenant 2");

        let legacy_order_id: i64 = sqlx::query_scalar(
            "INSERT INTO orders (order_number, customer_name, total_minor)
             VALUES ('TENANT-TEST-LEGACY', 'Test Customer', 1000)
             RETURNING id",
        )
        .fetch_one(&db)
        .await
        .expect("legacy order should default to tenant 1");

        let item_tenant: i64 =
            sqlx::query_scalar("SELECT restaurant_id FROM menu_items WHERE id = $1")
                .bind(default_item_id)
                .fetch_one(&db)
                .await
                .expect("read tenant assignment");
        let order_tenant: i64 =
            sqlx::query_scalar("SELECT restaurant_id FROM orders WHERE id = $1")
                .bind(legacy_order_id)
                .fetch_one(&db)
                .await
                .expect("read order tenant assignment");
        assert_eq!(item_tenant, 1);
        assert_eq!(order_tenant, 1);

        let cross_tenant_order_line = sqlx::query(
            "INSERT INTO order_lines (restaurant_id, order_id, menu_item_id, quantity, unit_price_minor)
             VALUES (1, $1, $2, 1, 2000)",
        )
        .bind(legacy_order_id)
        .bind(second_item_id)
        .execute(&db)
        .await;
        assert!(
            cross_tenant_order_line.is_err(),
            "database must reject order lines that reference another tenant's item"
        );

        let cross_tenant_category = sqlx::query(
            "INSERT INTO menu_items (restaurant_id, category_id, name, price_minor, available)
             VALUES ($1, $2, 'Invalid cross-tenant category', 100, TRUE)",
        )
        .bind(other_restaurant_id)
        .bind(default_category_id)
        .execute(&db)
        .await;
        assert!(
            cross_tenant_category.is_err(),
            "database must reject menu items that reference another tenant's category"
        );
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
        let chef_membership: (i64, String) = sqlx::query_as(
            "SELECT restaurant_id, role FROM restaurant_staff_memberships
             WHERE staff_user_id = $1 AND is_active = TRUE",
        )
        .bind(chef_id)
        .fetch_one(&db)
        .await
        .expect("CLI-created staff should receive a default restaurant membership");
        assert_eq!(chef_membership, (1, "chef".to_owned()));

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

    async fn staff_session_headers(state: &AppState, username: &str, password: &str) -> HeaderMap {
        let response = auth::login(
            State(state.clone()),
            test_origin_headers(),
            Json(auth::LoginRequest {
                username: username.to_owned(),
                password: password.to_owned(),
            }),
        )
        .await
        .expect("staff login should succeed");
        let cookie_pair = response
            .headers()
            .get(header::SET_COOKIE)
            .expect("login should set a cookie")
            .to_str()
            .expect("cookie header should be valid")
            .split(';')
            .next()
            .expect("cookie pair");
        let mut headers = test_origin_headers();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_bytes(cookie_pair.as_bytes()).expect("valid cookie pair"),
        );
        headers
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

        let saved_line: (i32, i64, i64, i64) = sqlx::query_as(
            "SELECT quantity, unit_price_minor, restaurant_id, order_id
             FROM order_lines WHERE order_id = $1",
        )
        .bind(created.id)
        .fetch_one(&db)
        .await
        .expect("persisted order line should exist");
        assert_eq!(saved_line, (2, 12_500, 1, created.id));

        let second_restaurant_id: i64 = sqlx::query_scalar(
            "INSERT INTO restaurants (slug, name, status)
             VALUES ($1, 'Cart Isolation Restaurant', 'active')
             RETURNING id",
        )
        .bind(format!("cart-test-{}", Uuid::new_v4().simple()))
        .fetch_one(&db)
        .await
        .expect("create second restaurant for cart test");
        let second_restaurant_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (restaurant_id, name, price_minor, available)
             VALUES ($1, 'Second Restaurant Dish', 7000, TRUE)
             RETURNING id",
        )
        .bind(second_restaurant_id)
        .fetch_one(&db)
        .await
        .expect("create second restaurant dish");

        assert!(matches!(
            submit(
                &db,
                "Integration Test",
                vec![
                    OrderItemInput {
                        menu_item_id: available_item_id,
                        quantity: 1,
                    },
                    OrderItemInput {
                        menu_item_id: second_restaurant_item_id,
                        quantity: 1,
                    },
                ],
            )
            .await,
            Err(ApiError::BadRequest(_))
        ));

        let filtered_items = list_menu_items(
            State(AppState {
                db: db.clone(),
                cookie_secure: false,
                csrf_allowed_origins: vec![],
            }),
            Query(RestaurantFilter {
                restaurant_id: Some(second_restaurant_id),
            }),
        )
        .await
        .expect("tenant-filtered menu query should work");
        assert_eq!(filtered_items.0.len(), 1);
        assert_eq!(filtered_items.0[0].restaurant_id, second_restaurant_id);
        let active_restaurants = list_restaurants(State(AppState {
            db: db.clone(),
            cookie_secure: false,
            csrf_allowed_origins: vec![],
        }))
        .await
        .expect("active restaurant directory should work");
        assert!(active_restaurants
            .0
            .iter()
            .any(|restaurant| restaurant.id == second_restaurant_id));

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
    #[tokio::test]
    async fn order_status_transitions_are_role_scoped_atomic_and_logged() {
        let _guard = TEST_DB_LOCK.lock().await;
        let db = test_database().await;
        let owner_password = "a-strong-owner-password";
        let chef_password = "a-strong-chef-password";
        let waiter_password = "a-strong-waiter-password";

        auth::bootstrap_owner(&db, "owner_01", owner_password)
            .await
            .expect("create owner");
        auth::create_staff(&db, "chef_01", chef_password, "chef")
            .await
            .expect("create chef");
        auth::create_staff(&db, "waiter_01", waiter_password, "waiter")
            .await
            .expect("create waiter");

        let item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (name, price_minor, available)
             VALUES ('Status test meal', 1000, TRUE)
             RETURNING id",
        )
        .fetch_one(&db)
        .await
        .expect("insert test item");
        let (_, Json(first_order)) = submit(
            &db,
            "Status test",
            vec![OrderItemInput {
                menu_item_id: item_id,
                quantity: 1,
            }],
        )
        .await
        .expect("create first order");
        let (_, Json(second_order)) = submit(
            &db,
            "Cancellation test",
            vec![OrderItemInput {
                menu_item_id: item_id,
                quantity: 1,
            }],
        )
        .await
        .expect("create second order");

        let other_restaurant_id: i64 = sqlx::query_scalar(
            "INSERT INTO restaurants (slug, name, status)
             VALUES ($1, 'Tenant Two', 'active')
             RETURNING id",
        )
        .bind(format!("status-test-{}", Uuid::new_v4().simple()))
        .fetch_one(&db)
        .await
        .expect("create tenant two");
        let other_item_id: i64 = sqlx::query_scalar(
            "INSERT INTO menu_items (restaurant_id, name, price_minor, available)
             VALUES ($1, 'Tenant Two Item', 2000, TRUE)
             RETURNING id",
        )
        .bind(other_restaurant_id)
        .fetch_one(&db)
        .await
        .expect("create tenant two menu item");
        let (_, Json(other_order)) = submit(
            &db,
            "Tenant Two Customer",
            vec![OrderItemInput {
                menu_item_id: other_item_id,
                quantity: 1,
            }],
        )
        .await
        .expect("create tenant two order");

        let state = AppState {
            db: db.clone(),
            cookie_secure: false,
            csrf_allowed_origins: vec!["http://localhost:8088".to_owned()],
        };
        let owner_headers = staff_session_headers(&state, "owner_01", owner_password).await;
        let chef_headers = staff_session_headers(&state, "chef_01", chef_password).await;
        let waiter_headers = staff_session_headers(&state, "waiter_01", waiter_password).await;

        assert!(matches!(
            update_order_status(
                State(state.clone()),
                chef_headers.clone(),
                Path(other_order.id),
                Json(UpdateOrderStatusRequest {
                    status: "accepted".to_owned(),
                }),
            )
            .await,
            Err(ApiError::Forbidden)
        ));

        let (_, _, Json(tenant_two_accepted)) = update_order_status(
            State(state.clone()),
            owner_headers.clone(),
            Path(other_order.id),
            Json(UpdateOrderStatusRequest {
                status: "accepted".to_owned(),
            }),
        )
        .await
        .expect("platform owner may update an order across tenants");
        assert_eq!(tenant_two_accepted.status, "accepted");

        let kitchen_response = list_kitchen_orders(State(state.clone()), chef_headers.clone())
            .await
            .expect("chef may read their restaurant orders");
        let kitchen_body = axum::body::to_bytes(kitchen_response.into_body(), usize::MAX)
            .await
            .expect("read kitchen response");
        let kitchen_json = String::from_utf8(kitchen_body.to_vec()).expect("UTF-8 JSON response");
        assert!(kitchen_json.contains(&first_order.order_number));
        assert!(!kitchen_json.contains(&other_order.order_number));

        let mut missing_origin = owner_headers.clone();
        missing_origin.remove(header::ORIGIN);
        assert!(matches!(
            update_order_status(
                State(state.clone()),
                missing_origin,
                Path(second_order.id),
                Json(UpdateOrderStatusRequest {
                    status: "cancelled".to_owned(),
                }),
            )
            .await,
            Err(ApiError::Forbidden)
        ));

        let (_, _, Json(accepted)) = update_order_status(
            State(state.clone()),
            owner_headers.clone(),
            Path(first_order.id),
            Json(UpdateOrderStatusRequest {
                status: "accepted".to_owned(),
            }),
        )
        .await
        .expect("owner should accept pending order");
        assert_eq!(accepted.status, "accepted");

        let (_, _, Json(preparing)) = update_order_status(
            State(state.clone()),
            chef_headers.clone(),
            Path(first_order.id),
            Json(UpdateOrderStatusRequest {
                status: "preparing".to_owned(),
            }),
        )
        .await
        .expect("chef should start preparation");
        assert_eq!(preparing.status, "preparing");

        let (_, _, Json(ready)) = update_order_status(
            State(state.clone()),
            chef_headers.clone(),
            Path(first_order.id),
            Json(UpdateOrderStatusRequest {
                status: "ready".to_owned(),
            }),
        )
        .await
        .expect("chef should mark order ready");
        assert_eq!(ready.status, "ready");

        let (_, _, Json(completed)) = update_order_status(
            State(state.clone()),
            waiter_headers,
            Path(first_order.id),
            Json(UpdateOrderStatusRequest {
                status: "completed".to_owned(),
            }),
        )
        .await
        .expect("waiter should complete ready order");
        assert_eq!(completed.status, "completed");

        assert!(matches!(
            update_order_status(
                State(state.clone()),
                owner_headers.clone(),
                Path(first_order.id),
                Json(UpdateOrderStatusRequest {
                    status: "preparing".to_owned(),
                }),
            )
            .await,
            Err(ApiError::Conflict(_))
        ));

        assert!(matches!(
            update_order_status(
                State(state.clone()),
                chef_headers,
                Path(second_order.id),
                Json(UpdateOrderStatusRequest {
                    status: "cancelled".to_owned(),
                }),
            )
            .await,
            Err(ApiError::Forbidden)
        ));

        let (_, _, Json(cancelled)) = update_order_status(
            State(state.clone()),
            owner_headers,
            Path(second_order.id),
            Json(UpdateOrderStatusRequest {
                status: "cancelled".to_owned(),
            }),
        )
        .await
        .expect("owner should cancel pending order");
        assert_eq!(cancelled.status, "cancelled");

        let events: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT order_id, from_status, to_status
             FROM order_status_events
             ORDER BY id",
        )
        .fetch_all(&db)
        .await
        .expect("fetch order status events");
        assert_eq!(
            events,
            vec![
                (other_order.id, "pending".to_owned(), "accepted".to_owned()),
                (first_order.id, "pending".to_owned(), "accepted".to_owned()),
                (
                    first_order.id,
                    "accepted".to_owned(),
                    "preparing".to_owned()
                ),
                (first_order.id, "preparing".to_owned(), "ready".to_owned()),
                (first_order.id, "ready".to_owned(), "completed".to_owned()),
                (
                    second_order.id,
                    "pending".to_owned(),
                    "cancelled".to_owned()
                ),
            ],
            "only valid transitions should be recorded",
        );
    }

    #[test]
    fn order_status_transition_roles_are_restricted() {
        assert!(role_can_update_order_status(
            "owner",
            "preparing",
            "cancelled"
        ));
        assert!(role_can_update_order_status(
            "restaurant_owner",
            "preparing",
            "cancelled"
        ));
        assert!(role_can_update_order_status(
            "manager",
            "ready",
            "completed"
        ));
        assert!(role_can_update_order_status("chef", "pending", "accepted"));
        assert!(role_can_update_order_status(
            "chef",
            "accepted",
            "preparing"
        ));
        assert!(role_can_update_order_status("chef", "preparing", "ready"));
        assert!(role_can_update_order_status("waiter", "ready", "completed"));
        assert!(role_can_update_order_status(
            "cashier",
            "accepted",
            "cancelled"
        ));

        assert!(!role_can_update_order_status(
            "chef",
            "pending",
            "cancelled"
        ));
        assert!(!role_can_update_order_status(
            "cashier",
            "ready",
            "completed"
        ));
        assert!(!role_can_update_order_status(
            "waiter",
            "preparing",
            "ready"
        ));
        assert!(!valid_order_transition("completed", "preparing"));
        assert!(!valid_order_transition("pending", "pending"));
    }
}
