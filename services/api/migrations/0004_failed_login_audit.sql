ALTER TABLE staff_audit_log
    DROP CONSTRAINT IF EXISTS staff_audit_log_event_type_check;

ALTER TABLE staff_audit_log
    ADD CONSTRAINT staff_audit_log_event_type_check
    CHECK (event_type IN (
        'owner_bootstrapped',
        'staff_provisioned_cli',
        'staff_login_succeeded',
        'staff_login_failed',
        'staff_logout'
    ));

ALTER TABLE staff_audit_log
    ALTER COLUMN subject_role DROP NOT NULL;

ALTER TABLE staff_audit_log
    DROP CONSTRAINT IF EXISTS staff_audit_log_subject_role_check;

ALTER TABLE staff_audit_log
    ADD CONSTRAINT staff_audit_log_subject_role_check
    CHECK (
        subject_role IS NULL
        OR subject_role IN ('owner', 'manager', 'cashier', 'chef', 'waiter')
    );

ALTER TABLE staff_audit_log
    ADD COLUMN attempted_username TEXT
    CHECK (
        attempted_username IS NULL
        OR attempted_username ~ '^[a-z0-9][a-z0-9._-]{2,31}$'
    );
