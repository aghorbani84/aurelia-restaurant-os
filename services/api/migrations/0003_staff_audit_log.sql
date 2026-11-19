CREATE TABLE staff_audit_log (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL
        CHECK (event_type IN (
            'owner_bootstrapped',
            'staff_provisioned_cli',
            'staff_login_succeeded',
            'staff_logout'
        )),
    actor_staff_user_id BIGINT
        REFERENCES staff_users(id) ON DELETE SET NULL,
    subject_staff_user_id BIGINT
        REFERENCES staff_users(id) ON DELETE SET NULL,
    subject_role TEXT NOT NULL
        CHECK (subject_role IN ('owner', 'manager', 'cashier', 'chef', 'waiter')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_staff_audit_log_created_at
    ON staff_audit_log (created_at DESC);

CREATE INDEX idx_staff_audit_log_subject_created_at
    ON staff_audit_log (subject_staff_user_id, created_at DESC);
