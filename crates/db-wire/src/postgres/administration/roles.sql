SELECT r.oid AS id,''::text AS schema,r.rolname::text AS name,''::text AS parent,
 format('login: %s; superuser: %s; create DB: %s; create role: %s; inherit: %s; replication: %s; bypass RLS: %s; connection limit: %s; member of: %s',
 r.rolcanlogin,r.rolsuper,r.rolcreatedb,r.rolcreaterole,r.rolinherit,r.rolreplication,r.rolbypassrls,r.rolconnlimit,
 coalesce((SELECT string_agg(quote_ident(pg_get_userbyid(m.roleid)),', ' ORDER BY m.roleid) FROM pg_auth_members m WHERE m.member=r.oid),'none')) AS details,
 format('ALTER ROLE %I %s %s CONNECTION LIMIT %s;',r.rolname,CASE WHEN r.rolcanlogin THEN 'LOGIN' ELSE 'NOLOGIN' END,
 CASE WHEN r.rolinherit THEN 'INHERIT' ELSE 'NOINHERIT' END,r.rolconnlimit) AS source,NULL::text AS invocation FROM pg_roles r
