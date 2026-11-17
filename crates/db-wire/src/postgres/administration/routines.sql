SELECT p.oid AS id, n.nspname::text AS schema, p.proname::text AS name,
 ''::text AS parent,
 format('%s(%s) → %s; owner: %s', CASE p.prokind WHEN 'p' THEN 'procedure' ELSE 'function' END,
 pg_get_function_identity_arguments(p.oid), pg_get_function_result(p.oid), pg_get_userbyid(p.proowner)) AS details,
 pg_get_functiondef(p.oid) AS source,
 format('%s %I.%I(%s);', CASE p.prokind WHEN 'p' THEN 'CALL' ELSE 'SELECT * FROM' END,
 n.nspname,p.proname,coalesce((SELECT string_agg(
 CASE WHEN p.provariadic<>0 AND a.ordinality=array_length(CASE WHEN p.prokind='p' THEN coalesce(p.proallargtypes,p.proargtypes::oid[]) ELSE p.proargtypes::oid[] END,1) THEN 'VARIADIC ' ELSE '' END
 || 'NULL::' || format_type(a.typ,NULL), ', ' ORDER BY a.ordinality)
 FROM unnest(CASE WHEN p.prokind='p' THEN coalesce(p.proallargtypes,p.proargtypes::oid[]) ELSE p.proargtypes::oid[] END)
 WITH ORDINALITY a(typ,ordinality)),'')) AS invocation
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE p.prokind IN ('f','p') AND n.nspname NOT IN ('pg_catalog','information_schema')
 AND n.nspname !~ '^pg_(toast|temp)'
