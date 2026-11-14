use axum::{
    extract::State,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{env, net::SocketAddr};
use tower_http::{cors::{AllowOrigin, CorsLayer}, trace::TraceLayer};
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

#[derive(Debug)]
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

fn cors_layer() -> CorsLayer {
    let configured_origins = env::var("CORS_ALLOWED_ORIGINS").unwrap_or_default();
    let origins = parse_allowed_origins(&configured_origins)
        .unwrap_or_else(|error| panic!("{error}"));

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

    let state = AppState { db };
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .route("/api/menu/categories", get(list_categories))
        .route("/api/menu/items", get(list_menu_items))
        .route("/api/orders", post(create_order))
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

    #[test]
    fn cors_allowlist_trims_entries_and_ignores_blanks() {
        let origins = parse_allowed_origins(
            " https://shop.example , ,http://localhost:5173 ",
        )
        .expect("valid exact origins");

        assert_eq!(
            origins,
            vec![
                HeaderValue::from_static("https://shop.example"),
                HeaderValue::from_static("http://localhost:5173"),
            ],
        );
    }

    #[test]
    fn cors_allowlist_rejects_wildcard_null_and_invalid_header_values() {
        assert!(parse_allowed_origins("*").is_err());
        assert!(parse_allowed_origins("null").is_err());
        assert!(parse_allowed_origins("https://shop.example\\nInjected: true").is_err());
    }

    async fn test_database() -> PgPool {
        let database_url = env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to a dedicated disposable test database");
        let db = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("connect to test PostgreSQL database");

        for statement in include_str!("../../../infra/db/init.sql").split(';') {
            let statement = statement.trim();
            if !statement.is_empty() {
                sqlx::query(statement)
                    .execute(&db)
                    .await
                    .expect("initialize test database schema");
            }
        }

        sqlx::query(
            "TRUNCATE TABLE order_lines, orders, menu_items, menu_categories RESTART IDENTITY CASCADE",
        )
        .execute(&db)
        .await
        .expect("reset dedicated test database");

        db
    }

    async fn submit(
        db: &PgPool,
        customer_name: &str,
        items: Vec<OrderItemInput>,
    ) -> Result<(StatusCode, Json<CreatedOrder>), ApiError> {
        create_order(
            State(AppState { db: db.clone() }),
            Json(CreateOrderRequest {
                customer_name: customer_name.to_owned(),
                items,
            }),
        )
        .await
    }

    #[tokio::test]
    async fn order_creation_uses_database_prices_and_rejects_invalid_orders() {
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
