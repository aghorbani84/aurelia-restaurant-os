# AURELIA

> A Persian-first, multi-restaurant ordering platform in early development: Rust API, React web app, PostgreSQL, and an OpenResty gateway.

[![CI](https://github.com/aghorbani84/aurelia-restaurant-os/actions/workflows/ci.yml/badge.svg)](https://github.com/aghorbani84/aurelia-restaurant-os/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/Rust-Axum%20%2B%20SQLx-000000?logo=rust)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-4169E1?logo=postgresql&logoColor=white)
![OpenResty](https://img.shields.io/badge/gateway-OpenResty-15A6D1)
![Status](https://img.shields.io/badge/status-early%20development-orange)

## Overview

AURELIA started as a single-restaurant system and is being extended into a multi-restaurant food marketplace (see [`docs/marketplace-foundation.md`](docs/marketplace-foundation.md)). It is a modular monolith: one Rust API behind a rate-limiting OpenResty gateway, a React/Vite web app, and PostgreSQL with versioned migrations.

> **Not production-ready.** Payments, customer accounts, logistics, a full kitchen display, inventory, reservations, accounting, currency configuration, and production deployment hardening are not implemented. Menu management is included in this branch, but its monetary unit is provisional and it has no browser end-to-end coverage.

## Features

What the code implements today:

- **Customer web app (Persian, RTL)** – restaurant discovery, menus, and guest ordering
- **Restaurant onboarding** – public application form at `/restaurant/apply`; the owner account stays inactive until an owner-role reviewer approves or rejects the application
- **Tenant-aware data** – restaurants, tenant-filtered menus, single-restaurant carts, and tenant-scoped staff order access
- **Staff menu management** – tenant-scoped category/item CRUD for active restaurant owners and managers, exact-Origin checks, atomic audit logging, and soft-disable to preserve order history
- **Session-bound staff tenant selection** – authenticated staff can list active memberships and select a restaurant per session; menu and order access is scoped to that selection and rejects cross-tenant writes
- **Server-side pricing** – order totals are computed from database prices in integer minor units, with overflow checks, in a single transaction
- **Staff authentication** – Argon2id password hashes, opaque session tokens stored as SHA-256 hashes, `HttpOnly` / `SameSite=Lax` / `Secure` cookies with a 12-hour lifetime
- **Role-based access** – `owner`, `manager`, `cashier`, `chef`, and `waiter` roles with separate admin, kitchen, and service order views
- **Order status workflow** – `pending → accepted → preparing → ready → completed`, with role-scoped transitions and cancellation from early states; every change is recorded in `order_status_events`
- **Audit log** – staff lifecycle events, failed logins, and restaurant-application decisions, with an owner-only paginated API and viewer in the `/staff` workspace
- **Request hardening** – exact-origin CORS allowlist (off by default), Origin-based CSRF checks on staff login/logout, menu mutations, restaurant selection, and order status changes, plus per-IP gateway rate limits (stricter for login and restaurant applications)

## Architecture

| Component | Tech | Location |
| --- | --- | --- |
| Web app | React 19, TypeScript, Vite | `apps/web` |
| API | Rust, Axum 0.7, SQLx 0.8, Tokio | `services/api` |
| Database | PostgreSQL 16, versioned SQLx migrations | `services/api/migrations` |
| Gateway | OpenResty / LuaJIT (rate limiting, `/api/` proxy) | `services/gateway` |
| Local stack | Docker Compose | `docker-compose.yml` |

The API listens on port `8080` inside the Docker network only and is reached through the gateway, so its rate limits cannot be bypassed from the host.

## Getting started

Requires Docker Engine with the Compose plugin.

```bash
git clone https://github.com/aghorbani84/aurelia-restaurant-os.git
cd aurelia-restaurant-os

# Use a non-default password even on a development machine.
export POSTGRES_PASSWORD='change-this-local-dev-password'
# Exact browser origins allowed for staff login/logout and status changes.
export CSRF_ALLOWED_ORIGINS='http://localhost:5173,http://localhost:8088'
# Optional: only when the browser calls the API from another origin.
# export CORS_ALLOWED_ORIGINS='https://app.example.com'
# Secure cookies are on by default; set false only for local plain-HTTP testing.
# export COOKIE_SECURE=false

docker compose up --build
```

| Service | Address | Purpose |
| --- | --- | --- |
| Web app | http://localhost:5173 | Vite dev server (customer marketplace) |
| Restaurant application | http://localhost:5173/restaurant/apply | Onboarding form |
| Staff workspace | http://localhost:5173/staff | Staff login, role-scoped orders, owner audit/application review, and menu management (tenant selector UI pending) |
| Gateway | http://localhost:8088 | Rate-limited API proxy |
| PostgreSQL | localhost:5432 | Development database |

The API applies migrations at startup. Add new migration files for schema changes; never edit an applied migration.

### Create the initial owner

There is no public staff-registration endpoint. Create the first owner through the one-time CLI command after the services are running. The command refuses to run if any staff account already exists.

```bash
export AURELIA_ADMIN_USERNAME=owner
read -rsp 'Initial owner password (12-128 bytes): ' AURELIA_ADMIN_PASSWORD
printf '\n'
export AURELIA_ADMIN_PASSWORD
docker compose exec \
  -e "AURELIA_ADMIN_USERNAME=$AURELIA_ADMIN_USERNAME" \
  -e "AURELIA_ADMIN_PASSWORD=$AURELIA_ADMIN_PASSWORD" \
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
  -e "AURELIA_STAFF_USERNAME=$AURELIA_STAFF_USERNAME" \
  -e "AURELIA_STAFF_PASSWORD=$AURELIA_STAFF_PASSWORD" \
  -e "AURELIA_STAFF_ROLE=$AURELIA_STAFF_ROLE" \
  api aurelia-api create-staff
unset AURELIA_STAFF_PASSWORD AURELIA_STAFF_USERNAME AURELIA_STAFF_ROLE
```

Allowed roles for this command are `manager`, `cashier`, `chef`, and `waiter`. A currently active owner account must already exist. Creating another owner is intentionally not supported by this command.

## API

| Method | Path | Access |
| --- | --- | --- |
| GET | `/health`, `/ready` | Public liveness / database readiness |
| GET | `/api/restaurants` | Public restaurant list |
| POST | `/api/restaurants/apply` | Public restaurant application (stricter rate limit) |
| GET | `/api/menu/categories`, `/api/menu/items` | Public menu |
| GET | `/api/restaurant/menu/categories`, `/api/restaurant/menu/items` | Authenticated restaurant owner/manager |
| POST, DELETE | `/api/restaurant/menu/categories`, `/api/restaurant/menu/categories/:category_id` | Active restaurant owner/manager; mutations validate Origin |
| POST, PUT, DELETE | `/api/restaurant/menu/items`, `/api/restaurant/menu/items/:item_id` | Active restaurant owner/manager; mutations validate Origin |
| PATCH | `/api/restaurant/menu/items/:item_id/availability` | Active restaurant owner/manager; validates Origin |
| POST | `/api/orders` | Public guest order |
| POST | `/api/orders/:order_id/status` | Staff session, role-scoped transition, CSRF Origin check |
| POST | `/api/auth/login`, `/api/auth/logout` | Staff login (10 req/min per IP at gateway) / logout |
| GET | `/api/auth/me` | Current staff session, including selected restaurant |
| GET | `/api/auth/restaurants` | Authenticated staff; active restaurant memberships |
| PUT | `/api/auth/restaurant` | Authenticated staff; exact-Origin check and active membership required |
| GET | `/api/admin/orders` | Owner, manager, cashier |
| GET | `/api/kitchen/orders` | Owner, manager, chef |
| GET | `/api/service/orders` | Owner, manager, waiter |
| GET | `/api/admin/audit?limit=50&before_id=…` | Owner |
| GET | `/api/admin/restaurants/applications` | Owner |
| POST | `/api/admin/restaurants/applications/:id/review` | Owner (`approve` / `reject`) |

Example order (clients never send prices):

```bash
curl -X POST http://localhost:8088/api/orders \
  -H 'Content-Type: application/json' \
  -d '{"customer_name": "Test Customer", "items": [{"menu_item_id": 1, "quantity": 2}]}'
```

## Testing

API integration tests need a **dedicated, disposable** PostgreSQL database; they clear AURELIA tables before running. Never point `TEST_DATABASE_URL` at a real database.

```bash
docker run --rm --name aurelia-test-db \
  -e POSTGRES_USER=aurelia -e POSTGRES_PASSWORD=aurelia_test -e POSTGRES_DB=aurelia_test \
  -p 5433:5432 postgres:16-alpine

# in another terminal
TEST_DATABASE_URL='postgres://aurelia:aurelia_test@localhost:5433/aurelia_test' \
  cargo test --manifest-path services/api/Cargo.toml
```

CI ([`ci.yml`](.github/workflows/ci.yml)) builds the web app, runs `cargo fmt --check`, `cargo check`, and `cargo test` against PostgreSQL 16, validates the Compose and OpenResty configuration, and runs a Docker smoke test of the staff auth flow (`scripts/smoke-auth.sh`).

## Project structure

```
├── apps/web/                 # React + Vite web app (customer, apply, staff)
├── services/
│   ├── api/                  # Rust Axum API, auth, migrations, Dockerfile
│   └── gateway/              # OpenResty config + Lua rate limiters
├── docs/marketplace-foundation.md
├── scripts/smoke-auth.sh
└── docker-compose.yml
```

## Current limitations

- No customer authentication, payments, or logistics; currency is not yet configured.
- No staff-management or role-change API or UI; staff are provisioned via the local CLI.
- The `/staff` workspace does not yet expose order status controls (the API supports them).
- Menu `price_minor` has no configured currency or minor-unit scale; the Toman label is provisional. Do not use menu prices for payments or accounting.
- No browser end-to-end test covers the staff menu manager or tenant-selection workflow.
- The React staff restaurant switcher is implemented in the follow-up branch stacked on the session API PR; it is not available on `main` until the stacked PRs are merged.
- The Compose web service is the Vite dev server, not a production frontend server.
- Audit-log retention and pruning are not implemented.

## Author

Built by [aghorbani84](https://github.com/aghorbani84).
