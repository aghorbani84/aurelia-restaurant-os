# AURELIA Restaurant OS

**AURELIA** is a Persian-first restaurant platform under active development. The current stack is:

- **Web:** React, Vite, Three.js
- **API:** Rust, Axum, SQLx
- **Database:** PostgreSQL 16
- **Gateway:** OpenResty / LuaJIT

> **Project status:** early development scaffold. The menu and order API now uses PostgreSQL, but authentication, authorization, kitchen display, inventory, reservations, accounting, production deployment hardening, and full end-to-end tests are not implemented yet.

## Run locally

Requirements: Docker Engine and Docker Compose plugin.

```bash
# Optional: set a local development password before starting
export POSTGRES_PASSWORD='change-this-local-dev-password'
# Optional: only needed when browsers access the API from a different origin
export CORS_ALLOWED_ORIGINS='https://app.example.com,https://admin.example.com'
docker compose up --build
```

Services:

| Service | Local address | Purpose |
|---|---|---|
| Web | http://localhost:5173 | Vite development frontend |
| OpenResty gateway | http://localhost:8088 | Gateway and rate-limited API proxy |
| Rust API | http://localhost:8080 | API and health endpoints |
| PostgreSQL | localhost:5432 | Local development database |

The Compose default database password is for local development only. Set `POSTGRES_PASSWORD` before using the stack beyond a local machine. Do not commit real credentials.

The schema in `infra/db/init.sql` is mounted into PostgreSQL's initialization directory. PostgreSQL executes it only when the data directory is first initialized. If you change the schema and need a clean local database, you can run `docker compose down -v` and then start the stack again. **This deletes the local PostgreSQL volume and all data in it.**

## API

The API listens on port `8080`. The OpenResty gateway listens on `8088` and proxies `/api/` to the API.

| Method | Path | Description |
|---|---|---|
| GET | `/health` | Process liveness |
| GET | `/ready` | PostgreSQL readiness check |
| GET | `/api/menu/categories` | List menu categories |
| GET | `/api/menu/items` | List menu items and availability |
| POST | `/api/orders` | Create an order transactionally |

Example: create an order (prices are calculated by the API from database values; clients must not submit prices).

```bash
curl -X POST http://localhost:8088/api/orders \
  -H 'Content-Type: application/json' \
  -d '{
    "customer_name": "Test Customer",
    "items": [
      { "menu_item_id": 1, "quantity": 2 }
    ]
  }'
```

The `price_minor` and `total_minor` fields represent integer monetary minor units. The application still needs an explicit currency configuration before real payments or accounting are enabled.

Order creation validates customer name and line quantities, checks menu item availability in PostgreSQL, reads prices from the database, computes totals with overflow checks, and inserts the order and its lines in one transaction. An unavailable item or invalid request is rejected; database details are not returned to clients.

## Current limitations

- Cross-origin API access is disabled by default. Set `CORS_ALLOWED_ORIGINS` to a comma-separated list of exact origins when a separate browser origin is required; wildcard (`*`) and `null` origins are rejected. The local Vite proxy uses same-origin requests and does not need CORS.
- There is no customer/admin authentication or role-based authorization yet.
- No payment integration is present.
- The web UI is still a starter and is not yet connected to all API workflows.
- The initial SQL script is for a fresh local database; a versioned migration workflow is still needed.
- CI results must be checked on the relevant GitHub Actions run; a pushed commit alone is not evidence of a successful build.

## Development

Rust API source: `services/api`

Database bootstrap schema: `infra/db/init.sql`

Gateway configuration: `services/gateway/nginx.conf`

Web app: `apps/web`


## API integration tests

The order API integration test connects to a **dedicated disposable PostgreSQL database** and clears its AURELIA tables before running. Never point `TEST_DATABASE_URL` at a development or production database.

Example using a temporary PostgreSQL container:

```bash
docker run --rm --name aurelia-test-db \
  -e POSTGRES_USER=aurelia \
  -e POSTGRES_PASSWORD=aurelia_test \
  -e POSTGRES_DB=aurelia_test \
  -p 5433:5432 postgres:16-alpine

# In another terminal:
TEST_DATABASE_URL='postgres://aurelia:aurelia_test@localhost:5433/aurelia_test' \
  cargo test --manifest-path services/api/Cargo.toml
```

GitHub Actions provisions an isolated PostgreSQL 16 service and runs `cargo fmt --check`, `cargo check`, and `cargo test`.
