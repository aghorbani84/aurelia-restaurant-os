-- Marketplace tenant foundation. Existing single-restaurant data is assigned to the
-- deterministic legacy tenant (id=1). Keep all changes additive/backfill-first.
CREATE TABLE restaurants (
    id BIGSERIAL PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE
        CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}$'),
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 160),
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('draft', 'pending_review', 'approved', 'active', 'suspended')),
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO restaurants (id, slug, name, status, is_default)
VALUES (1, 'legacy-default', 'AURELIA Legacy Restaurant', 'active', TRUE);

SELECT setval('restaurants_id_seq', (SELECT MAX(id) FROM restaurants), TRUE);

CREATE UNIQUE INDEX restaurants_single_default_idx
    ON restaurants (is_default) WHERE is_default;

CREATE TABLE restaurant_staff_memberships (
    id BIGSERIAL PRIMARY KEY,
    restaurant_id BIGINT NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    staff_user_id BIGINT NOT NULL REFERENCES staff_users(id) ON DELETE CASCADE,
    role TEXT NOT NULL
        CHECK (role IN ('restaurant_owner', 'manager', 'cashier', 'chef', 'waiter')),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (restaurant_id, staff_user_id)
);

CREATE INDEX idx_restaurant_staff_memberships_user
    ON restaurant_staff_memberships (staff_user_id, is_active);
CREATE INDEX idx_restaurant_staff_memberships_restaurant
    ON restaurant_staff_memberships (restaurant_id, role, is_active);

-- Preserve existing staff access to the legacy restaurant while platform owners
-- remain platform-wide and are not represented as restaurant memberships.
INSERT INTO restaurant_staff_memberships (restaurant_id, staff_user_id, role)
SELECT 1, id, role
FROM staff_users
WHERE role IN ('manager', 'cashier', 'chef', 'waiter')
ON CONFLICT (restaurant_id, staff_user_id) DO NOTHING;

ALTER TABLE menu_categories
    ADD COLUMN restaurant_id BIGINT NOT NULL DEFAULT 1
        REFERENCES restaurants(id) ON DELETE CASCADE;
ALTER TABLE menu_categories
    DROP CONSTRAINT IF EXISTS menu_categories_slug_key;
ALTER TABLE menu_categories
    ADD CONSTRAINT menu_categories_restaurant_slug_unique
        UNIQUE (restaurant_id, slug),
    ADD CONSTRAINT menu_categories_restaurant_id_id_unique
        UNIQUE (restaurant_id, id);

ALTER TABLE menu_items
    ADD COLUMN restaurant_id BIGINT NOT NULL DEFAULT 1
        REFERENCES restaurants(id) ON DELETE CASCADE,
    ADD CONSTRAINT menu_items_restaurant_id_id_unique
        UNIQUE (restaurant_id, id);
ALTER TABLE menu_items
    DROP CONSTRAINT IF EXISTS menu_items_category_id_fkey;
ALTER TABLE menu_items
    ADD CONSTRAINT menu_items_restaurant_category_fk
        FOREIGN KEY (restaurant_id, category_id)
        REFERENCES menu_categories (restaurant_id, id)
        ON DELETE SET NULL (category_id);

ALTER TABLE orders
    ADD COLUMN restaurant_id BIGINT NOT NULL DEFAULT 1
        REFERENCES restaurants(id) ON DELETE RESTRICT,
    ADD CONSTRAINT orders_restaurant_id_id_unique
        UNIQUE (restaurant_id, id);

ALTER TABLE order_lines
    ADD COLUMN restaurant_id BIGINT NOT NULL DEFAULT 1
        REFERENCES restaurants(id) ON DELETE RESTRICT,
;
UPDATE order_lines ol
SET restaurant_id = o.restaurant_id
FROM orders o
WHERE ol.order_id = o.id;

ALTER TABLE order_lines
    DROP CONSTRAINT IF EXISTS order_lines_order_id_fkey,
    DROP CONSTRAINT IF EXISTS order_lines_menu_item_id_fkey;
ALTER TABLE order_lines
    ADD CONSTRAINT order_lines_restaurant_order_fk
        FOREIGN KEY (restaurant_id, order_id)
        REFERENCES orders (restaurant_id, id) ON DELETE CASCADE,
    ADD CONSTRAINT order_lines_restaurant_item_fk
        FOREIGN KEY (restaurant_id, menu_item_id)
        REFERENCES menu_items (restaurant_id, id) ON DELETE RESTRICT;

ALTER TABLE order_status_events
    ADD COLUMN restaurant_id BIGINT NOT NULL DEFAULT 1
        REFERENCES restaurants(id) ON DELETE RESTRICT;
UPDATE order_status_events e
SET restaurant_id = o.restaurant_id
FROM orders o
WHERE e.order_id = o.id;
ALTER TABLE order_status_events
    DROP CONSTRAINT IF EXISTS order_status_events_order_id_fkey;
ALTER TABLE order_status_events
    ADD CONSTRAINT order_status_events_restaurant_order_fk
        FOREIGN KEY (restaurant_id, order_id)
        REFERENCES orders (restaurant_id, id) ON DELETE RESTRICT;

CREATE INDEX idx_restaurants_status_slug
    ON restaurants (status, slug);
CREATE INDEX idx_menu_items_restaurant_available
    ON menu_items (restaurant_id, available);
CREATE INDEX idx_orders_restaurant_status_created
    ON orders (restaurant_id, status, created_at DESC);
CREATE INDEX idx_order_lines_restaurant_order
    ON order_lines (restaurant_id, order_id);
