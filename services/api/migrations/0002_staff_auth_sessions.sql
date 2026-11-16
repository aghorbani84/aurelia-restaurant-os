CREATE TABLE staff_users (
    id BIGSERIAL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE
        CHECK (username = lower(username))
        CHECK (username ~ '^[a-z0-9][a-z0-9._-]{2,31}$'),
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL
        CHECK (role IN ('owner', 'manager', 'cashier', 'chef', 'waiter')),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE staff_sessions (
    token_hash BYTEA PRIMARY KEY
        CHECK (octet_length(token_hash) = 32),
    staff_user_id BIGINT NOT NULL
        REFERENCES staff_users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ
);

CREATE INDEX idx_staff_sessions_user_expiry
    ON staff_sessions (staff_user_id, expires_at)
    WHERE revoked_at IS NULL;
