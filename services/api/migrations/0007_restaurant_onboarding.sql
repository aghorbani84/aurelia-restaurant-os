ALTER TABLE restaurants
    ADD COLUMN description TEXT NOT NULL DEFAULT '',
    ADD COLUMN contact_phone TEXT NOT NULL DEFAULT '',
    ADD COLUMN contact_email TEXT NOT NULL DEFAULT '',
    ADD COLUMN city TEXT NOT NULL DEFAULT '',
    ADD COLUMN address TEXT NOT NULL DEFAULT '',
    ADD COLUMN cuisine TEXT NOT NULL DEFAULT '',
    ADD COLUMN delivery_mode TEXT NOT NULL DEFAULT 'restaurant'
        CHECK (delivery_mode IN ('restaurant', 'marketplace', 'third_party')),
    ADD COLUMN delivery_fee_minor BIGINT NOT NULL DEFAULT 0
        CHECK (delivery_fee_minor >= 0),
    ADD COLUMN minimum_order_minor BIGINT NOT NULL DEFAULT 0
        CHECK (minimum_order_minor >= 0),
    ADD COLUMN estimated_prep_minutes SMALLINT NOT NULL DEFAULT 20
        CHECK (estimated_prep_minutes BETWEEN 5 AND 180);

ALTER TABLE restaurants
    DROP CONSTRAINT IF EXISTS restaurants_status_check,
    ADD CONSTRAINT restaurants_status_check
        CHECK (status IN ('draft', 'pending_review', 'approved', 'active', 'suspended', 'rejected'));

CREATE TABLE restaurant_applications (
    id BIGSERIAL PRIMARY KEY,
    restaurant_id BIGINT NOT NULL UNIQUE REFERENCES restaurants(id) ON DELETE CASCADE,
    owner_staff_user_id BIGINT NOT NULL REFERENCES staff_users(id) ON DELETE RESTRICT,
    contact_name TEXT NOT NULL CHECK (char_length(contact_name) BETWEEN 1 AND 120),
    status TEXT NOT NULL DEFAULT 'pending_review'
        CHECK (status IN ('pending_review', 'approved', 'rejected')),
    reviewed_by_staff_user_id BIGINT REFERENCES staff_users(id) ON DELETE SET NULL,
    reviewed_at TIMESTAMPTZ,
    review_note TEXT NOT NULL DEFAULT '',
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_restaurant_applications_status_submitted
    ON restaurant_applications (status, submitted_at DESC);

ALTER TABLE staff_audit_log
    ADD COLUMN restaurant_id BIGINT REFERENCES restaurants(id) ON DELETE SET NULL;

ALTER TABLE staff_audit_log
    DROP CONSTRAINT IF EXISTS staff_audit_log_event_type_check,
    ADD CONSTRAINT staff_audit_log_event_type_check
        CHECK (event_type IN (
            'owner_bootstrapped',
            'staff_provisioned_cli',
            'staff_login_succeeded',
            'staff_logout',
            'restaurant_application_submitted',
            'restaurant_application_approved',
            'restaurant_application_rejected'
        ));

ALTER TABLE staff_audit_log
    DROP CONSTRAINT IF EXISTS staff_audit_log_subject_role_check,
    ADD CONSTRAINT staff_audit_log_subject_role_check
        CHECK (subject_role IN ('owner', 'manager', 'cashier', 'chef', 'waiter', 'restaurant_owner'));

CREATE INDEX idx_staff_audit_log_restaurant_created
    ON staff_audit_log (restaurant_id, created_at DESC);
