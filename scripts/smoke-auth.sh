#!/usr/bin/env bash
set -euo pipefail

export POSTGRES_PASSWORD="aurelia-smoke-password"
export COOKIE_SECURE=false

tmp_dir="$(mktemp -d)"
cleanup() {
  docker compose down -v --remove-orphans || true
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

docker compose up -d --build db api gateway

ready=0
for _ in $(seq 1 60); do
  if curl -fsS http://localhost:8088/api/menu/items >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  docker compose logs
  echo "API did not become ready through the gateway" >&2
  exit 1
fi

export AURELIA_ADMIN_USERNAME=owner
export AURELIA_ADMIN_PASSWORD="aurelia-owner-password-123"
docker compose exec -T \
  -e AURELIA_ADMIN_USERNAME \
  -e AURELIA_ADMIN_PASSWORD \
  api aurelia-api bootstrap-owner
unset AURELIA_ADMIN_USERNAME AURELIA_ADMIN_PASSWORD

curl -fsS \
  -c "$tmp_dir/owner.cookies" \
  -H "Content-Type: application/json" \
  -d '{"username":"owner","password":"aurelia-owner-password-123"}' \
  http://localhost:8088/api/auth/login > "$tmp_dir/owner-login.json"

curl -fsS -b "$tmp_dir/owner.cookies" \
  http://localhost:8088/api/auth/me > "$tmp_dir/owner-me.json"
grep -q '"role":"owner"' "$tmp_dir/owner-me.json"

item_id="$(docker compose exec -T db psql -U aurelia -d aurelia -Atq \
  -c "INSERT INTO menu_items (name, price_minor, available) VALUES ('Smoke Meal', 1250, TRUE) RETURNING id;")"

curl -fsS \
  -H "Content-Type: application/json" \
  -d "{\"customer_name\":\"Smoke Test\",\"items\":[{\"menu_item_id\":$item_id,\"quantity\":2}]}" \
  http://localhost:8088/api/orders > "$tmp_dir/order.json"
grep -q '"total_minor":2500' "$tmp_dir/order.json"

curl -fsS -b "$tmp_dir/owner.cookies" \
  http://localhost:8088/api/admin/orders > "$tmp_dir/admin-orders.json"
grep -q '"customer_name":"Smoke Test"' "$tmp_dir/admin-orders.json"

export AURELIA_STAFF_USERNAME=chef
export AURELIA_STAFF_PASSWORD="aurelia-chef-password-123"
export AURELIA_STAFF_ROLE=chef
docker compose exec -T \
  -e AURELIA_STAFF_USERNAME \
  -e AURELIA_STAFF_PASSWORD \
  -e AURELIA_STAFF_ROLE \
  api aurelia-api create-staff
unset AURELIA_STAFF_USERNAME AURELIA_STAFF_PASSWORD AURELIA_STAFF_ROLE

curl -fsS \
  -c "$tmp_dir/chef.cookies" \
  -H "Content-Type: application/json" \
  -d '{"username":"chef","password":"aurelia-chef-password-123"}' \
  http://localhost:8088/api/auth/login > "$tmp_dir/chef-login.json"

curl -fsS -b "$tmp_dir/chef.cookies" \
  http://localhost:8088/api/kitchen/orders > "$tmp_dir/kitchen-orders.json"
grep -q '"customer_name":"Smoke Test"' "$tmp_dir/kitchen-orders.json"

admin_status="$(curl -sS -o "$tmp_dir/chef-admin.json" -w '%{http_code}' \
  -b "$tmp_dir/chef.cookies" http://localhost:8088/api/admin/orders)"
if [ "$admin_status" != "403" ]; then
  echo "Chef role should be forbidden from admin orders; received HTTP $admin_status" >&2
  exit 1
fi

logout_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -b "$tmp_dir/owner.cookies" -c "$tmp_dir/owner.cookies" \
  -X POST http://localhost:8088/api/auth/logout)"
if [ "$logout_status" != "204" ]; then
  echo "Expected logout HTTP 204; received HTTP $logout_status" >&2
  exit 1
fi

revoked_status="$(curl -sS -o "$tmp_dir/revoked.json" -w '%{http_code}' \
  -b "$tmp_dir/owner.cookies" http://localhost:8088/api/auth/me)"
if [ "$revoked_status" != "401" ]; then
  echo "Revoked owner session should return HTTP 401; received HTTP $revoked_status" >&2
  exit 1
fi

echo "AURELIA Docker smoke test passed."
