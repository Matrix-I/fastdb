WITH candidates AS (
    SELECT c.oid, c.relname::text AS name,
           CASE WHEN c.relkind = 'm' THEN 'materialized view' ELSE 'view' END AS kind,
           ''::text AS relation
    FROM pg_class c
    WHERE c.relnamespace = $1 AND c.relkind IN ('v', 'm')
    UNION ALL
    SELECT p.oid, p.proname::text, 'routine', ''
    FROM pg_proc p
    WHERE p.pronamespace = $1 AND p.prokind IN ('f', 'p')
    UNION ALL
    SELECT t.oid, t.tgname::text, 'trigger', c.relname::text
    FROM pg_class c JOIN pg_trigger t ON t.tgrelid = c.oid
    WHERE c.relnamespace = $1 AND NOT t.tgisinternal
    UNION ALL
    SELECT r.oid, r.rulename::text, 'rule', c.relname::text
    FROM pg_class c JOIN pg_rewrite r ON r.ev_class = c.oid
    WHERE c.relnamespace = $1 AND r.rulename <> '_RETURN'
    UNION ALL
    SELECT i.indexrelid, ic.relname::text, 'index', c.relname::text
    FROM pg_class c JOIN pg_index i ON i.indrelid = c.oid
    JOIN pg_class ic ON ic.oid = i.indexrelid
    WHERE c.relnamespace = $1
    UNION ALL
    SELECT k.oid, k.conname::text, 'constraint', c.relname::text
    FROM pg_class c JOIN pg_constraint k ON k.conrelid = c.oid
    WHERE c.relnamespace = $1
    UNION ALL
    SELECT k.oid, k.conname::text, 'constraint', d.typname::text
    FROM pg_type d JOIN pg_constraint k ON k.contypid = d.oid
    WHERE d.typnamespace = $1 AND d.typtype = 'd'
    UNION ALL
    SELECT p.oid, p.polname::text, 'policy', c.relname::text
    FROM pg_class c JOIN pg_policy p ON p.polrelid = c.oid
    WHERE c.relnamespace = $1
    UNION ALL
    SELECT s.oid, s.stxname::text, 'statistics', ''
    FROM pg_statistic_ext s
    WHERE s.stxnamespace = $1
    -- Keep namespace filtering ahead of the expensive deparser calls.
    OFFSET 0
), matches AS (
    SELECT oid, name, kind, relation
    FROM candidates
    WHERE strpos(lower(
        CASE kind
            WHEN 'routine' THEN pg_get_functiondef(oid)
            WHEN 'trigger' THEN pg_get_triggerdef(oid)
            WHEN 'rule' THEN pg_get_ruledef(oid)
            WHEN 'index' THEN pg_get_indexdef(oid)
            WHEN 'constraint' THEN pg_get_constraintdef(oid)
            WHEN 'statistics' THEN pg_get_statisticsobjdef(oid)
            WHEN 'policy' THEN (
                SELECT concat_ws(E'\n',
                    'USING (' || pg_get_expr(p.polqual, p.polrelid) || ')',
                    'WITH CHECK (' || pg_get_expr(p.polwithcheck, p.polrelid) || ')')
                FROM pg_policy p WHERE p.oid = candidates.oid
            )
            ELSE pg_get_viewdef(oid, false)
        END
    ), lower($2)) > 0
    ORDER BY oid, kind
    LIMIT $3
)
SELECT oid, name, kind,
       CASE WHEN kind = 'routine' THEN pg_get_function_identity_arguments(oid) ELSE relation END,
       CASE WHEN octet_length(definition) <= $4 THEN definition END
FROM matches
CROSS JOIN LATERAL (
    SELECT CASE kind
               WHEN 'routine' THEN pg_get_functiondef(oid)
               WHEN 'trigger' THEN pg_get_triggerdef(oid)
               WHEN 'rule' THEN pg_get_ruledef(oid)
               WHEN 'index' THEN pg_get_indexdef(oid)
               WHEN 'constraint' THEN pg_get_constraintdef(oid)
               WHEN 'statistics' THEN pg_get_statisticsobjdef(oid)
               WHEN 'policy' THEN (
                   SELECT concat_ws(E'\n',
                       'USING (' || pg_get_expr(p.polqual, p.polrelid) || ')',
                       'WITH CHECK (' || pg_get_expr(p.polwithcheck, p.polrelid) || ')')
                   FROM pg_policy p WHERE p.oid = matches.oid
               )
               ELSE pg_get_viewdef(oid, false)
           END AS definition
) AS source
ORDER BY oid, kind
