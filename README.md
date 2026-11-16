# AURELIA Restaurant OS

**AURELIA** is a Persian-first restaurant platform under active development. The current stack is:

- **Web:** React, Vite, Three.js
- **API:** Rust, Axum, SQLx
- **Database:** PostgreSQL 16
- **Gateway:** OpenResty / LuaJIT

> **Project status:** early development. PostgreSQL-backed menu and order APIs, staff login sessions, and role-scoped read-only order endpoints exist. The staff-management UI, order mutations, full kitchen display, inventory, reservations, accounting, payments, production deployment hardening, and full end-to-end browser tests are not implemented.

## Run locally

Requirements: Docker Engine and Docker Compose plugin.

```bash
# Use a non-default password even for a shared development machine.
export POSTGRES_PASSWORD='change-this-local-dev-password'
# Optional: only needed when browsers access the API from a different origin.
export CORS_ALLOWED_ORIGINS='https://app.example.com,https://admin.example.com'
# Secure cookies are enabled by default. Disable only for local plain-HTTP testing.
export COOKIE_SECURE=true
docker compose up --build
```

Services:

| Service | Local address | Purpose |
|---|---|---|
| Web | http://localhost:5173 | Vite development frontend |
| OpenResty gateway | http://localhost:8088 | Gateway and rate-limited API proxy |
| Rust API | Internal Docker network only | API; not published on the host |
| PostgreSQL | localhost:5432 | Local development database |

The Compose default database password is for local development only. Set `POSTGRES_PASSWORD` before using the stack beyond a local machine. Do not commit real credentials. The API is intentionally not published directly to the host so the gateway's rate limits cannot be bypassed through the Compose host port.

The API applies versioned SQLx migrations at startup and records applied versions in PostgreSQL. The initial migration uses idempotent table creation so an existing local volume initialized by the previous bootstrap SQL keeps its current tables and data. Future schema changes must be added as new migration files; do not edit an already-applied migration.

## Create the initial owner

There is no public staff-registration endpoint. Create the first owner through the one-time CLI command after the services are running. The command refuses to run if any staff account already exists.

```bash
export AURELIA_ADMIN_USERNAME=owner
read -rsp 'Initial owner password (12-128 bytes): ' AURELIA_ADMIN_PASSWORD
printf '\n'
export AURELIA_ADMIN_PASSWORD
docker compose exec \
  -e AURELIA_ADMIN_USERNAME \
  -e AURELIA_ADMIN_PASSWORD \
  api aurelia-api bootstrap-owner
unset AURELIA_ADMIN_PASSWORD AURELIA_ADMIN_USERNAME
```

Usernames are normalized to lowercase ASCII and must be 3-32 characters using letters, digits, dots, underscores, or hyphens. Initial passwords must be 12-128 bytes. Passwords are stored as Argon2id hashes. The bootstrap command is serialized with a PostgreSQL advisory transaction lock and can create only the first staff account.

Additional staff accounts can be provisioned by an operator with Docker access. This is a local administrative command, not a public API:

```bash
export AURELIA_STAFF_USERNAME=chef_01
export AURELIA_STAFF_ROLE=chef
read -rsp 'Staff password (12-128 bytes): ' AURELIA_STAFF_PASSWORD
printf '\n'
export AURELIA_STAFF_PASSWORD
docker compose exec \
  -e AURELIA_STAFF_USERNAME \
  -e AURELIA_STAFF_PASSWORD \
  -e AURELIA_STAFF_ROLE \
  api aurelia-api create-staff
unset AURELIA_STAFF_PASSWORD AURELIA_STAFF_USERNAME AURELIA_STAFF_ROLE
```

Allowed roles for this command are `manager`, `cashier`, `chef`, and `waiter`. A currently active owner account must already exist. Creating another owner is intentionally not supported by this command.

## API

The Rust API listens on port `8080` inside the Docker network. The OpenResty gateway listens on `8088` and proxies `/api/` to the API.

| Method | Path | Access |
|---|---|---|
| GET | `/health` | Public process liveness |
| GET | `/ready` | Public PostgreSQL readiness |
| GET | `/api/menu/categories` | Public menu |
| GET | `/api/menu/items` | Public menu |
| POST | `/api/orders` | Public customer order request |
| POST | `/api/auth/login` | Staff login |
| POST | `/api/auth/logout` | Current staff session |
| GET | `/api/auth/me` | Current staff session |
| GET | `/api/admin/orders` | Owner, manager, cashier |
| GET | `/api/kitchen/orders` | Owner, manager, chef |
| GET | `/api/service/orders` | Owner, manager, waiter |

Login creates a random 256-bit opaque session token. Only its SHA-256 hash is stored in PostgreSQL. The cookie is host-only, `HttpOnly`, `SameSite=Lax`, has a 12-hour lifetime, and is `Secure` by default. Logout revokes the server-side session and expires the cookie. Set `COOKIE_SECURE=false` only for local plain-HTTP testing; never disable it in an HTTPS deployment.

The login endpoint is limited to 10 requests per minute per client IP at the OpenResty gateway. Other `/api/` requests are limited to 120 per minute per client IP. The API is not host-published in Compose; keep it behind the gateway in this development topology.

## Role boundaries

- `owner`: admin, kitchen, and service order views.
- `manager`: admin, kitchen, and service order views.
- `cashier`: admin order view.
- `chef`: kitchen order view.
- `waiter`: service order view.

These are currently read-only order-summary endpoints. They are not a complete KDS or staff-management system. No staff-creation or role-change API is exposed yet. The initial owner and additional staff accounts are provisioned through the local Docker CLI; role changes are not exposed through an API.

The current staff-protected routes are read-only. Before adding state-changing staff endpoints, implement and test CSRF protection, audit logging, and explicit per-action permissions. CORS is not a substitute for CSRF protection.

## Orders and money

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
- There is no customer authentication.
- Staff account creation is available only through the local Docker CLI; no web staff-management UI or public staff-creation API exists.
- Order status mutations, payment integration, full KDS, inventory, reservations, and accounting are not implemented.
- The web UI is still a starter and is not yet connected to staff login or role-specific workspaces.
- The Compose web service is the Vite development server, not a production frontend server.
- Schema changes are managed through versioned SQLx migrations in `services/api/migrations`.
- CI results must be checked on the relevant GitHub Actions run; a pushed commit alone is not evidence of a successful build.

## Development

- Rust API source: `services/api`
- Database migrations: `services/api/migrations`
- Gateway configuration: `services/gateway/nginx.conf`
- Web app: `apps/web`

## API integration tests

The API integration tests connect to a **dedicated disposable PostgreSQL database** and clear its AURELIA tables before running. Never point `TEST_DATABASE_URL` at a development or production database.

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
