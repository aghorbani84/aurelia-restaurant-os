use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{env, net::SocketAddr};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: PgPool,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
}

enum ApiError {
    BadRequest(&'static str),
    NotFound(&'static str),
    Conflict(&'static str),
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
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

async fn list_menu_items(
    State(state): State<AppState>,
) -> Result<Json<Vec<MenuItem>>, ApiError> {
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

#[tokio::main]
async fn main() {
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL environment variable is required");

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("failed to connect to PostgreSQL");

    let state = AppState { db };
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .route("/api/menu/categories", get(list_categories))
        .route("/api/menu/items", get(list_menu_items))
        .route("/api/orders", post(create_order))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let address: SocketAddr = "0.0.0.0:8080".parse().expect("valid bind address");
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("failed to bind API listener");
    axum::serve(listener, app)
        .await
        .expect("API server failed");
}
