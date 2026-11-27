ALTER TABLE staff_sessions
    ADD COLUMN selected_restaurant_id BIGINT
        REFERENCES restaurants(id) ON DELETE SET NULL;

ALTER TABLE staff_audit_log
    DROP CONSTRAINT IF EXISTS staff_audit_log_event_type_check,
    ADD CONSTRAINT staff_audit_log_event_type_check
        CHECK (event_type IN (
            'owner_bootstrapped',
            'staff_provisioned_cli',
            'staff_login_succeeded',
            'staff_login_failed',
            'staff_logout',
            'restaurant_application_submitted',
            'restaurant_application_approved',
            'restaurant_application_rejected',
            'restaurant_menu_category_created',
            'restaurant_menu_category_deleted',
            'restaurant_menu_item_created',
            'restaurant_menu_item_updated',
            'restaurant_menu_item_deleted',
            'staff_restaurant_selected'
        ));
