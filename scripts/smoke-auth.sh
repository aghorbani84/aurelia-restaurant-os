#!/usr/bin/env bash
set -euo pipefail

export POSTGRES_PASSWORD="aurelia-smoke-password"
export COOKIE_SECURE=false
export CSRF_ALLOWED_ORIGINS=http://localhost:8088

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
  -e "AURELIA_ADMIN_USERNAME=$AURELIA_ADMIN_USERNAME" \
  -e "AURELIA_ADMIN_PASSWORD=$AURELIA_ADMIN_PASSWORD" \
  api aurelia-api bootstrap-owner
unset AURELIA_ADMIN_USERNAME AURELIA_ADMIN_PASSWORD

curl -fsS \
  -c "$tmp_dir/owner.cookies" \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:8088" \
  -d '{"username":"owner","password":"aurelia-owner-password-123"}' \
  http://localhost:8088/api/auth/login > "$tmp_dir/owner-login.json"

curl -fsS -b "$tmp_dir/owner.cookies" \
  http://localhost:8088/api/auth/me > "$tmp_dir/owner-me.json"
grep -q '"role":"owner"' "$tmp_dir/owner-me.json"

failed_login_status="$(curl -sS -o "$tmp_dir/failed-login.json" -w '%{http_code}' \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:8088" \
  -d '{"username":"intruder_01","password":"incorrect-password"}' \
  http://localhost:8088/api/auth/login)"
if [ "$failed_login_status" != "401" ]; then
  echo "Invalid staff credentials should return HTTP 401; received HTTP $failed_login_status" >&2
  exit 1
fi

curl -fsS -b "$tmp_dir/owner.cookies" \
  http://localhost:8088/api/admin/audit?limit=50 > "$tmp_dir/audit.json"
grep -q '"event_type":"staff_login_failed"' "$tmp_dir/audit.json"
grep -q '"attempted_username":"intruder_01"' "$tmp_dir/audit.json"

item_id="$(docker compose exec -T db psql -U aurelia -d aurelia -Atq \
  -c "INSERT INTO menu_items (name, price_minor, available) VALUES ('Smoke Meal', 1250, TRUE) RETURNING id;")"

curl -fsS \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:8088" \
  -d "{\"customer_name\":\"Smoke Test\",\"items\":[{\"menu_item_id\":$item_id,\"quantity\":2}]}" \
  http://localhost:8088/api/orders > "$tmp_dir/order.json"
grep -q '"total_minor":2500' "$tmp_dir/order.json"
order_id="$(jq -r .id "$tmp_dir/order.json")"

curl -fsS -b "$tmp_dir/owner.cookies" \
  http://localhost:8088/api/admin/orders > "$tmp_dir/admin-orders.json"
grep -q '"customer_name":"Smoke Test"' "$tmp_dir/admin-orders.json"

export AURELIA_STAFF_USERNAME=chef
export AURELIA_STAFF_PASSWORD="aurelia-chef-password-123"
export AURELIA_STAFF_ROLE=chef
docker compose exec -T \
  -e "AURELIA_STAFF_USERNAME=$AURELIA_STAFF_USERNAME" \
  -e "AURELIA_STAFF_PASSWORD=$AURELIA_STAFF_PASSWORD" \
  -e "AURELIA_STAFF_ROLE=$AURELIA_STAFF_ROLE" \
  api aurelia-api create-staff
unset AURELIA_STAFF_USERNAME AURELIA_STAFF_PASSWORD AURELIA_STAFF_ROLE

curl -fsS \
  -c "$tmp_dir/chef.cookies" \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:8088" \
  -d '{"username":"chef","password":"aurelia-chef-password-123"}' \
  http://localhost:8088/api/auth/login > "$tmp_dir/chef-login.json"

curl -fsS -b "$tmp_dir/chef.cookies" \
  http://localhost:8088/api/kitchen/orders > "$tmp_dir/kitchen-orders.json"
grep -q '"customer_name":"Smoke Test"' "$tmp_dir/kitchen-orders.json"

for next_status in accepted preparing ready; do
  curl -fsS \
    -b "$tmp_dir/chef.cookies" \
    -H "Content-Type: application/json" \
    -H "Origin: http://localhost:8088" \
    -d "{\"status\":\"$next_status\"}" \
    "http://localhost:8088/api/orders/$order_id/status" > "$tmp_dir/status-$next_status.json"
  grep -q "\"status\":\"$next_status\"" "$tmp_dir/status-$next_status.json"
done

export AURELIA_STAFF_USERNAME=waiter
export AURELIA_STAFF_PASSWORD="aurelia-waiter-password-123"
export AURELIA_STAFF_ROLE=waiter
docker compose exec -T \
  -e "AURELIA_STAFF_USERNAME=$AURELIA_STAFF_USERNAME" \
  -e "AURELIA_STAFF_PASSWORD=$AURELIA_STAFF_PASSWORD" \
  -e "AURELIA_STAFF_ROLE=$AURELIA_STAFF_ROLE" \
  api aurelia-api create-staff
unset AURELIA_STAFF_USERNAME AURELIA_STAFF_PASSWORD AURELIA_STAFF_ROLE

curl -fsS \
  -c "$tmp_dir/waiter.cookies" \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:8088" \
  -d '{"username":"waiter","password":"aurelia-waiter-password-123"}' \
  http://localhost:8088/api/auth/login > "$tmp_dir/waiter-login.json"

curl -fsS \
  -b "$tmp_dir/waiter.cookies" \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:8088" \
  -d '{"status":"completed"}' \
  "http://localhost:8088/api/orders/$order_id/status" > "$tmp_dir/status-completed.json"
grep -q '"status":"completed"' "$tmp_dir/status-completed.json"

invalid_transition_status="$(curl -sS -o "$tmp_dir/invalid-transition.json" -w '%{http_code}' \
  -b "$tmp_dir/owner.cookies" \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:8088" \
  -d '{"status":"preparing"}' \
  "http://localhost:8088/api/orders/$order_id/status")"
if [ "$invalid_transition_status" != "409" ]; then
  echo "Completed order must not move back to preparing; received HTTP $invalid_transition_status" >&2
  exit 1
fi

admin_status="$(curl -sS -o "$tmp_dir/chef-admin.json" -w '%{http_code}' \
  -b "$tmp_dir/chef.cookies" http://localhost:8088/api/admin/orders)"
if [ "$admin_status" != "403" ]; then
  echo "Chef role should be forbidden from admin orders; received HTTP $admin_status" >&2
  exit 1
fi

audit_status="$(curl -sS -o "$tmp_dir/chef-audit.json" -w '%{http_code}' \
  -b "$tmp_dir/chef.cookies" http://localhost:8088/api/admin/audit)"
if [ "$audit_status" != "403" ]; then
  echo "Chef role should be forbidden from audit records; received HTTP $audit_status" >&2
  exit 1
fi

logout_status="$(curl -sS -o /dev/null -w '%{http_code}' \
  -b "$tmp_dir/owner.cookies" -c "$tmp_dir/owner.cookies" \
  -H "Origin: http://localhost:8088" \
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
