SELECT col.table_schema, col.table_name, col.column_name, col.data_type,
       col.is_nullable, col.column_default, col.collation_name,
       format_type(a.atttypid, a.atttypmod), col_description(c.oid, a.attnum),
       EXISTS(SELECT 1 FROM pg_constraint k WHERE k.conrelid = c.oid
              AND k.contype = 'p' AND a.attnum = ANY(k.conkey)),
       EXISTS(SELECT 1 FROM pg_constraint k WHERE k.conrelid = c.oid
              AND k.contype = 'f' AND a.attnum = ANY(k.conkey)),
       col.ordinal_position
FROM information_schema.columns col
JOIN pg_namespace n ON n.nspname = col.table_schema
JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = col.table_name
JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = col.column_name
WHERE col.table_schema NOT IN ('information_schema', 'pg_catalog')
  AND ($1::text IS NULL OR col.table_schema = $1)
  AND ($2::text IS NULL OR col.table_name = $2)
UNION ALL
SELECT n.nspname::text, c.relname::text, a.attname::text,
       format_type(CASE WHEN ty.typtype = 'd' THEN ty.typbasetype ELSE a.atttypid END, NULL),
       CASE WHEN a.attnotnull OR (ty.typtype = 'd' AND ty.typnotnull) THEN 'NO' ELSE 'YES' END,
       NULL::text, co.collname::text,
       format_type(a.atttypid, a.atttypmod), col_description(c.oid, a.attnum),
       false, false, a.attnum::integer
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN pg_attribute a ON a.attrelid = c.oid
JOIN pg_type ty ON ty.oid = a.atttypid
LEFT JOIN pg_collation co ON co.oid = a.attcollation
WHERE c.relkind = 'm' AND a.attnum > 0 AND NOT a.attisdropped
  AND n.nspname NOT IN ('information_schema', 'pg_catalog')
  AND ($1::text IS NULL OR n.nspname = $1)
  AND ($2::text IS NULL OR c.relname = $2)
  AND (pg_has_role(c.relowner, 'USAGE')
       OR has_column_privilege(c.oid, a.attnum, 'SELECT'))
ORDER BY 1, 2, 12
