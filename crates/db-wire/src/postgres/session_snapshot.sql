WITH sessions AS MATERIALIZED (
    SELECT pid, datname::text, usename::text, application_name, state,
           wait_event_type, wait_event, backend_start,
           CASE WHEN state = 'active' THEN query_start END AS active_since,
           CASE WHEN state = 'active' THEN left(query, 4096) END AS active_query
    FROM pg_catalog.pg_stat_activity
    WHERE (backend_type = 'client backend' OR (backend_type IS NULL AND usename IS NOT NULL))
      AND pid <> pg_catalog.pg_backend_pid()
    ORDER BY (wait_event_type = 'Lock') DESC NULLS LAST, pid
    LIMIT $1
)
SELECT pid, datname, usename, application_name, state, wait_event_type, wait_event,
       backend_start, active_since, active_query,
       CASE WHEN active_since IS NOT NULL THEN
           GREATEST(0, (EXTRACT(epoch FROM (pg_catalog.statement_timestamp() - active_since)) * 1000)::bigint)
       END AS elapsed_ms,
       CASE WHEN wait_event_type = 'Lock' THEN (pg_catalog.pg_blocking_pids(pid))[1:$2]
            ELSE ARRAY[]::integer[] END AS blockers
FROM sessions
ORDER BY (wait_event_type = 'Lock') DESC NULLS LAST, pid
