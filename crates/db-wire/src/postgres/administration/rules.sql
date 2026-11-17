SELECT r.oid AS id,n.nspname::text AS schema,r.rulename::text AS name,format('%I.%I',n.nspname,c.relname) AS parent,
 format('relation: %I.%I; enabled: %s',n.nspname,c.relname,r.ev_enabled) AS details,
 regexp_replace(pg_get_ruledef(r.oid,true),'^CREATE RULE','CREATE OR REPLACE RULE') AS source,
 NULL::text AS invocation FROM pg_rewrite r JOIN pg_class c ON c.oid=r.ev_class JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE r.rulename<>'_RETURN' AND n.nspname !~ '^pg_(toast|temp)'
