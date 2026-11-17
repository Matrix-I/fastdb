SELECT p.oid AS id,n.nspname::text AS schema,p.polname::text AS name,format('%I.%I',n.nspname,c.relname) AS parent,
 format('table: %I.%I; command: %s; permissive: %s; RLS enabled: %s; RLS forced: %s',n.nspname,c.relname,p.polcmd,p.polpermissive,c.relrowsecurity,c.relforcerowsecurity) AS details,
 format('ALTER POLICY %I ON %I.%I TO %s%s%s;',p.polname,n.nspname,c.relname,
 (SELECT string_agg(CASE WHEN roleid=0 THEN 'PUBLIC' ELSE quote_ident(pg_get_userbyid(roleid)) END,', ' ORDER BY roleid) FROM unnest(p.polroles) roleid),
 CASE WHEN p.polqual IS NULL THEN '' ELSE ' USING (' || pg_get_expr(p.polqual,p.polrelid) || ')' END,
 CASE WHEN p.polwithcheck IS NULL THEN '' ELSE ' WITH CHECK (' || pg_get_expr(p.polwithcheck,p.polrelid) || ')' END) AS source,
 NULL::text AS invocation FROM pg_policy p JOIN pg_class c ON c.oid=p.polrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname !~ '^pg_(toast|temp)'
