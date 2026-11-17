SELECT c.oid AS id,n.nspname::text AS schema,c.relname::text AS name,
 CASE WHEN c.relispartition THEN format('%I.%I',pn.nspname,pc.relname) ELSE '' END AS parent,
 format('partition key: %s; bound: %s; owner: %s',coalesce(pg_get_partkeydef(c.oid),'not partitioned'),
 coalesce(pg_get_expr(c.relpartbound,c.oid),'root'),pg_get_userbyid(c.relowner)) AS details,
 NULL::text AS source,NULL::text AS invocation FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 LEFT JOIN pg_inherits i ON i.inhrelid=c.oid AND c.relispartition
 LEFT JOIN pg_class pc ON pc.oid=i.inhparent LEFT JOIN pg_namespace pn ON pn.oid=pc.relnamespace
 WHERE (c.relkind='p' OR c.relispartition) AND n.nspname !~ '^pg_(toast|temp)'
