CREATE TABLE order_status_events (
    id BIGSERIAL PRIMARY KEY,
    order_id BIGINT NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
    actor_staff_user_id BIGINT REFERENCES staff_users(id) ON DELETE SET NULL,
    from_status TEXT NOT NULL
        CHECK (from_status IN ('pending', 'accepted', 'preparing', 'ready', 'completed', 'cancelled')),
    to_status TEXT NOT NULL
        CHECK (to_status IN ('pending', 'accepted', 'preparing', 'ready', 'completed', 'cancelled')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (from_status <> to_status)
);

CREATE INDEX idx_order_status_events_order_created
    ON order_status_events (order_id, created_at DESC);
