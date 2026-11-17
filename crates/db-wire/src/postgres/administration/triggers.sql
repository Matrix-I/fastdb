SELECT t.oid AS id,n.nspname::text AS schema,t.tgname::text AS name,format('%I.%I',n.nspname,c.relname) AS parent,
 format('table: %I.%I; enabled: %s',n.nspname,c.relname,t.tgenabled) AS details,
 regexp_replace(pg_get_triggerdef(t.oid,true),'^CREATE TRIGGER','CREATE OR REPLACE TRIGGER') || ';' AS source,
 NULL::text AS invocation FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE NOT t.tgisinternal AND n.nspname !~ '^pg_(toast|temp)'
