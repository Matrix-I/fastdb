SELECT CASE WHEN $3::boolean THEN pg_catalog.pg_cancel_backend(pid)
            ELSE pg_catalog.pg_terminate_backend(pid) END AS signal_sent
FROM pg_catalog.pg_stat_activity
WHERE pid = $1
  AND backend_start = $2
  AND backend_type = 'client backend'
  AND pid <> pg_catalog.pg_backend_pid()
  AND (NOT $3 OR (state = 'active' AND query_start = $4))
