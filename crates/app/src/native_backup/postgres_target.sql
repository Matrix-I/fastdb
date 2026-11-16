SELECT CASE
    WHEN EXISTS (
        SELECT 1
        FROM pg_catalog.pg_stat_activity
        WHERE datname = pg_catalog.current_database()
          AND pid <> pg_catalog.pg_backend_pid()
          AND backend_type = 'client backend'
          AND application_name IS DISTINCT FROM 'FastDB Native Restore'
    ) THEN 'busy'
    WHEN EXISTS (
        SELECT 1
        FROM pg_catalog.pg_namespace
        WHERE nspname NOT IN ('public', 'pg_catalog', 'pg_toast', 'information_schema')
          AND nspname !~ '^pg_(temp|toast_temp)_[0-9]+$'
    ) OR EXISTS (
        SELECT 1
        FROM pg_catalog.pg_depend
        WHERE refclassid = 'pg_catalog.pg_namespace'::pg_catalog.regclass
          AND refobjid = (
              SELECT oid FROM pg_catalog.pg_namespace WHERE nspname = 'public'
          )
    ) OR EXISTS (
        SELECT 1 FROM pg_catalog.pg_extension WHERE extname <> 'plpgsql'
    ) OR EXISTS (
        SELECT 1 FROM pg_catalog.pg_largeobject_metadata
    ) OR EXISTS (
        SELECT 1 FROM pg_catalog.pg_publication
    ) OR EXISTS (
        SELECT 1 FROM pg_catalog.pg_stat_subscription
    ) OR EXISTS (
        SELECT 1 FROM pg_catalog.pg_foreign_data_wrapper
    ) OR EXISTS (
        SELECT 1 FROM pg_catalog.pg_event_trigger
    ) OR EXISTS (
        SELECT 1 FROM pg_catalog.pg_default_acl
    ) OR EXISTS (
        SELECT 1 FROM pg_catalog.pg_transform
    ) OR EXISTS (
        -- Normal object OIDs start after the bootstrap and reserved OID ranges.
        SELECT 1 FROM pg_catalog.pg_cast WHERE oid >= 16384
    ) THEN 'populated'
    WHEN NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = 'public'
    ) THEN 'modified'
    ELSE 'empty'
END;
