SELECT c.oid AS id,n.nspname::text AS schema,c.relname::text AS name,''::text AS parent,
 format('owner: %s; explicit ACL (NULL means native defaults): %s',pg_get_userbyid(c.relowner),coalesce(c.relacl::text,'NULL')) AS details,
 NULL::text AS source,NULL::text AS invocation FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE c.relkind IN ('r','p','v','m','f') AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_(toast|temp)'
