-- One catalog snapshot; server-side quoting also handles newlines and quotes in names.
WITH relations AS (
    SELECT c.*, n.nspname,
           format('%I.%I', n.nspname, c.relname) AS target,
           CASE WHEN c.relkind = 'v' THEN 'VIEW' ELSE 'TABLE' END AS object_type,
           EXISTS (SELECT FROM pg_inherits h WHERE h.inhrelid = c.oid) AND NOT c.relispartition
               OR c.reloftype <> 0 OR c.relkind NOT IN ('r', 'p', 'v')
               OR c.relpersistence = 't' AS unsupported
    FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
    JOIN information_schema.tables t ON t.table_schema = n.nspname AND t.table_name = c.relname
    WHERE n.nspname NOT IN ('information_schema', 'pg_catalog')
), sequences AS (
    SELECT d.refobjid AS table_oid, a.attnum, d.deptype, sn.nspname, s.relname,
           format('%I.%I', sn.nspname, s.relname) AS target,
           format_type(q.seqtypid, NULL) AS data_type,
           format(' INCREMENT BY %s MINVALUE %s MAXVALUE %s START WITH %s CACHE %s %s',
               q.seqincrement, q.seqmin, q.seqmax,
               q.seqstart, q.seqcache, CASE WHEN q.seqcycle THEN 'CYCLE' ELSE 'NO CYCLE' END) AS options,
           obj_description(s.oid, 'pg_class') AS comment,
           pg_get_userbyid(s.relowner) AS owner, s.relpersistence, s.relacl, s.relowner
    FROM pg_depend d JOIN pg_class s ON s.oid = d.objid AND s.relkind = 'S'
    JOIN pg_namespace sn ON sn.oid = s.relnamespace
    JOIN pg_sequence q ON q.seqrelid = s.oid
    JOIN pg_attribute a ON a.attrelid = d.refobjid AND a.attnum = d.refobjsubid
    JOIN relations r ON r.oid = d.refobjid
    WHERE d.classid = 'pg_class'::regclass AND d.refclassid = 'pg_class'::regclass
      AND d.deptype IN ('a', 'i')
), columns AS (
    SELECT a.attrelid, a.attnum,
        format('    %I %s', a.attname, format_type(a.atttypid, a.atttypmod)) ||
        CASE WHEN a.attcollation <> 0 AND a.attcollation <> ty.typcollation
             THEN format(' COLLATE %I.%I', cn.nspname, co.collname) ELSE '' END ||
        CASE WHEN a.attidentity <> '' THEN
            ' GENERATED ' || CASE a.attidentity WHEN 'a' THEN 'ALWAYS' ELSE 'BY DEFAULT' END ||
            ' AS IDENTITY (' || COALESCE((SELECT 'SEQUENCE NAME ' || s.target || s.options
                FROM sequences s WHERE s.table_oid = a.attrelid AND s.attnum = a.attnum AND s.deptype = 'i'), '') || ')'
        WHEN a.attgenerated <> '' THEN
            ' GENERATED ALWAYS AS (' || pg_get_expr(ad.adbin, ad.adrelid, false) || ')' ||
            CASE a.attgenerated WHEN 'v' THEN ' VIRTUAL' ELSE ' STORED' END
        WHEN ad.oid IS NOT NULL THEN ' DEFAULT ' || pg_get_expr(ad.adbin, ad.adrelid, false)
        ELSE '' END || CASE WHEN a.attnotnull THEN ' NOT NULL' ELSE '' END AS definition
    FROM pg_attribute a JOIN relations r ON r.oid = a.attrelid
    JOIN pg_type ty ON ty.oid = a.atttypid
    LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
    LEFT JOIN pg_collation co ON co.oid = a.attcollation
    LEFT JOIN pg_namespace cn ON cn.oid = co.collnamespace
    WHERE a.attnum > 0 AND NOT a.attisdropped
), statements AS (
    SELECT r.oid, 0 AS phase, '' AS sort_key,
        '-- PostgreSQL structural DDL. Dependencies must already exist; sequence values and data are not included.' || E'\n' ||
        'SET search_path = ' || COALESCE((SELECT string_agg(quote_ident(s), ', ' ORDER BY pos) FROM unnest(current_schemas(false)) WITH ORDINALITY path(s, pos)), 'pg_catalog') || E';\n' ||
        'SET standard_conforming_strings = on;' AS sql
    FROM relations r WHERE NOT r.unsupported
    UNION ALL
    SELECT r.oid, 0, '', '-- DDL unavailable: inheritance, typed, temporary or foreign tables are not supported.'
    FROM relations r WHERE r.unsupported
    UNION ALL
    SELECT s.table_oid, 10, s.target, 'CREATE ' || CASE WHEN s.relpersistence = 'u' THEN 'UNLOGGED ' ELSE '' END ||
        'SEQUENCE ' || s.target || ' AS ' || s.data_type || s.options || ';'
    FROM sequences s JOIN relations r ON r.oid = s.table_oid WHERE s.deptype = 'a' AND NOT r.unsupported
    UNION ALL
    SELECT r.oid, 20, '',
        CASE WHEN r.relkind = 'v' THEN 'CREATE VIEW ' || r.target || ' (' ||
            (SELECT string_agg(quote_ident(a.attname), ', ' ORDER BY a.attnum) FROM pg_attribute a
             WHERE a.attrelid = r.oid AND a.attnum > 0 AND NOT a.attisdropped) || ') AS ' || pg_get_viewdef(r.oid, false)
        ELSE 'CREATE ' || CASE WHEN r.relpersistence = 'u' THEN 'UNLOGGED ' ELSE '' END ||
            'TABLE ' || r.target || E' (\n' ||
            COALESCE((SELECT string_agg(c.definition, E',\n' ORDER BY c.attnum) FROM columns c WHERE c.attrelid = r.oid), '') || E'\n)' ||
            CASE WHEN r.relkind = 'p' THEN ' PARTITION BY ' || pg_get_partkeydef(r.oid) ELSE '' END ||
            COALESCE((SELECT ' USING ' || quote_ident(am.amname) FROM pg_am am WHERE am.oid = r.relam AND r.relkind = 'r'), '') ||
            COALESCE((SELECT ' TABLESPACE ' || quote_ident(ts.spcname) FROM pg_tablespace ts WHERE ts.oid = r.reltablespace), '') || ';'
        END
    FROM relations r WHERE NOT r.unsupported
    UNION ALL
    SELECT r.oid, 25, '', 'ALTER ' || r.object_type || ' ' || r.target || ' SET (' ||
        (SELECT string_agg(format('%I = %L', split_part(opt, '=', 1), substr(opt, strpos(opt, '=') + 1)), ', ' ORDER BY opt)
         FROM unnest(r.reloptions) opt) || ');'
    FROM relations r WHERE NOT r.unsupported AND array_length(r.reloptions, 1) > 0
    UNION ALL
    SELECT r.oid, 26, a.attname, 'ALTER VIEW ' || r.target || ' ALTER COLUMN ' || quote_ident(a.attname) ||
        ' SET DEFAULT ' || pg_get_expr(ad.adbin, ad.adrelid, false) || ';'
    FROM relations r JOIN pg_attribute a ON a.attrelid = r.oid
    JOIN pg_attrdef ad ON ad.adrelid = r.oid AND ad.adnum = a.attnum
    WHERE NOT r.unsupported AND r.relkind = 'v' AND a.attnum > 0 AND NOT a.attisdropped
    UNION ALL
    SELECT s.table_oid, 30, s.target, 'ALTER SEQUENCE ' || s.target || ' OWNED BY ' || r.target || '.' || quote_ident(a.attname) || ';'
    FROM sequences s JOIN relations r ON r.oid = s.table_oid
    JOIN pg_attribute a ON a.attrelid = r.oid AND a.attnum = s.attnum
    WHERE s.deptype = 'a' AND NOT r.unsupported
    UNION ALL
    SELECT r.oid, CASE WHEN con.contype IN ('p', 'u', 'x') THEN 39 ELSE 40 END, con.conname, 'ALTER TABLE ONLY ' || r.target || ' ADD CONSTRAINT ' ||
        quote_ident(con.conname) || ' ' || pg_get_constraintdef(con.oid, false) || ';'
    FROM relations r JOIN pg_constraint con ON con.conrelid = r.oid
    WHERE NOT r.unsupported AND con.contype IN ('p', 'u', 'c', 'x', 'f') AND con.conparentid = 0
    UNION ALL
    SELECT r.oid, 50, ic.relname,
        CASE WHEN NOT i.indisvalid OR NOT i.indisready THEN '-- WARNING: invalid index omitted: ' ||
             replace(replace(quote_ident(ic.relname), E'\n', ' '), E'\r', ' ')
        ELSE pg_get_indexdef(i.indexrelid, 0, false) || ';' END
    FROM relations r JOIN pg_index i ON i.indrelid = r.oid JOIN pg_class ic ON ic.oid = i.indexrelid
    WHERE NOT r.unsupported AND NOT EXISTS (SELECT FROM pg_constraint con WHERE con.conindid = i.indexrelid AND con.contype IN ('p', 'u', 'x'))
    UNION ALL
    SELECT r.oid, 51, ic.relname, 'ALTER TABLE ' || r.target || ' CLUSTER ON ' || quote_ident(ic.relname) || ';'
    FROM relations r JOIN pg_index i ON i.indrelid = r.oid JOIN pg_class ic ON ic.oid = i.indexrelid
    WHERE NOT r.unsupported AND i.indisvalid AND i.indisclustered
    UNION ALL
    SELECT r.oid, 55, '', 'ALTER TABLE ' || format('%I.%I', pn.nspname, p.relname) ||
        ' ATTACH PARTITION ' || r.target || ' ' || pg_get_expr(r.relpartbound, r.oid, false) || ';'
    FROM relations r JOIN pg_inherits h ON h.inhrelid = r.oid
    JOIN pg_class p ON p.oid = h.inhparent JOIN pg_namespace pn ON pn.oid = p.relnamespace
    WHERE NOT r.unsupported AND r.relispartition
    UNION ALL
    SELECT r.oid, 60, a.attname, format('COMMENT ON COLUMN %s.%I IS %L;', r.target, a.attname, col_description(r.oid, a.attnum))
    FROM relations r JOIN pg_attribute a ON a.attrelid = r.oid
    WHERE NOT r.unsupported AND a.attnum > 0 AND NOT a.attisdropped AND col_description(r.oid, a.attnum) IS NOT NULL
    UNION ALL
    SELECT r.oid, 60, '', format('COMMENT ON %s %s IS %L;', r.object_type, r.target, obj_description(r.oid, 'pg_class'))
    FROM relations r WHERE NOT r.unsupported AND obj_description(r.oid, 'pg_class') IS NOT NULL
    UNION ALL
    SELECT r.oid, 60, con.conname, format('COMMENT ON CONSTRAINT %I ON %s IS %L;', con.conname, r.target, obj_description(con.oid, 'pg_constraint'))
    FROM relations r JOIN pg_constraint con ON con.conrelid = r.oid
    WHERE NOT r.unsupported AND obj_description(con.oid, 'pg_constraint') IS NOT NULL
    UNION ALL
    SELECT r.oid, 60, ic.relname, format('COMMENT ON INDEX %I.%I IS %L;', n.nspname, ic.relname, obj_description(ic.oid, 'pg_class'))
    FROM relations r JOIN pg_index i ON i.indrelid = r.oid JOIN pg_class ic ON ic.oid = i.indexrelid
    JOIN pg_namespace n ON n.oid = ic.relnamespace
    WHERE NOT r.unsupported AND i.indisvalid AND obj_description(ic.oid, 'pg_class') IS NOT NULL
    UNION ALL
    SELECT s.table_oid, 60, s.target, format('COMMENT ON SEQUENCE %s IS %L;', s.target, s.comment)
    FROM sequences s JOIN relations r ON r.oid = s.table_oid WHERE NOT r.unsupported AND s.comment IS NOT NULL
    UNION ALL
    SELECT r.oid, 65, a.attname, 'ALTER TABLE ONLY ' || r.target || ' ALTER COLUMN ' || quote_ident(a.attname) ||
        ' SET STORAGE ' || CASE a.attstorage WHEN 'p' THEN 'PLAIN' WHEN 'e' THEN 'EXTERNAL' WHEN 'm' THEN 'MAIN' ELSE 'EXTENDED' END || ';' ||
        CASE WHEN a.attstattarget >= 0 THEN E'\nALTER TABLE ONLY ' || r.target || ' ALTER COLUMN ' || quote_ident(a.attname) ||
             ' SET STATISTICS ' || a.attstattarget || ';' ELSE '' END ||
        CASE WHEN a.attcompression <> '' THEN E'\nALTER TABLE ONLY ' || r.target || ' ALTER COLUMN ' || quote_ident(a.attname) ||
             ' SET COMPRESSION ' || CASE a.attcompression WHEN 'p' THEN 'pglz' ELSE 'lz4' END || ';' ELSE '' END
    FROM relations r JOIN pg_attribute a ON a.attrelid = r.oid
    WHERE NOT r.unsupported AND r.relkind IN ('r', 'p') AND a.attnum > 0 AND NOT a.attisdropped AND a.attgenerated <> 'v'
    UNION ALL
    SELECT r.oid, 70, p.polname, 'CREATE POLICY ' || quote_ident(p.polname) || ' ON ' || r.target ||
        CASE WHEN p.polpermissive THEN ' AS PERMISSIVE' ELSE ' AS RESTRICTIVE' END ||
        ' FOR ' || CASE p.polcmd WHEN 'r' THEN 'SELECT' WHEN 'a' THEN 'INSERT' WHEN 'w' THEN 'UPDATE' WHEN 'd' THEN 'DELETE' ELSE 'ALL' END ||
        ' TO ' || (SELECT string_agg(CASE WHEN role = 0 THEN 'PUBLIC' ELSE quote_ident(pg_get_userbyid(role)) END, ', ' ORDER BY role) FROM unnest(p.polroles) role) ||
        COALESCE(' USING (' || pg_get_expr(p.polqual, p.polrelid, false) || ')', '') ||
        COALESCE(' WITH CHECK (' || pg_get_expr(p.polwithcheck, p.polrelid, false) || ')', '') || ';' ||
        CASE WHEN obj_description(p.oid, 'pg_policy') IS NOT NULL THEN E'\n' || format('COMMENT ON POLICY %I ON %s IS %L;', p.polname, r.target, obj_description(p.oid, 'pg_policy')) ELSE '' END
    FROM relations r JOIN pg_policy p ON p.polrelid = r.oid WHERE NOT r.unsupported
    UNION ALL
    SELECT r.oid, 75, t.tgname, pg_get_triggerdef(t.oid, false) || ';' ||
        CASE t.tgenabled WHEN 'D' THEN E'\nALTER TABLE ' || r.target || ' DISABLE TRIGGER ' || quote_ident(t.tgname) || ';'
        WHEN 'R' THEN E'\nALTER TABLE ' || r.target || ' ENABLE REPLICA TRIGGER ' || quote_ident(t.tgname) || ';'
        WHEN 'A' THEN E'\nALTER TABLE ' || r.target || ' ENABLE ALWAYS TRIGGER ' || quote_ident(t.tgname) || ';' ELSE '' END ||
        CASE WHEN obj_description(t.oid, 'pg_trigger') IS NOT NULL THEN E'\n' || format('COMMENT ON TRIGGER %I ON %s IS %L;', t.tgname, r.target, obj_description(t.oid, 'pg_trigger')) ELSE '' END
    FROM relations r JOIN pg_trigger t ON t.tgrelid = r.oid WHERE NOT r.unsupported AND NOT t.tgisinternal AND t.tgparentid = 0
    UNION ALL
    SELECT r.oid, 76, rw.rulename, pg_get_ruledef(rw.oid, false) ||
        CASE rw.ev_enabled WHEN 'D' THEN E'\nALTER TABLE ' || r.target || ' DISABLE RULE ' || quote_ident(rw.rulename) || ';'
        WHEN 'R' THEN E'\nALTER TABLE ' || r.target || ' ENABLE REPLICA RULE ' || quote_ident(rw.rulename) || ';'
        WHEN 'A' THEN E'\nALTER TABLE ' || r.target || ' ENABLE ALWAYS RULE ' || quote_ident(rw.rulename) || ';' ELSE '' END
    FROM relations r JOIN pg_rewrite rw ON rw.ev_class = r.oid WHERE NOT r.unsupported AND rw.rulename <> '_RETURN'
    UNION ALL
    SELECT r.oid, 77, st.stxname, pg_get_statisticsobjdef(st.oid) || ';' ||
        CASE WHEN st.stxstattarget >= 0 THEN E'\nALTER STATISTICS ' || format('%I.%I', n.nspname, st.stxname) || ' SET STATISTICS ' || st.stxstattarget || ';' ELSE '' END ||
        E'\nALTER STATISTICS ' || format('%I.%I', n.nspname, st.stxname) || ' OWNER TO ' || quote_ident(pg_get_userbyid(st.stxowner)) || ';' ||
        CASE WHEN obj_description(st.oid, 'pg_statistic_ext') IS NOT NULL THEN E'\n' || format('COMMENT ON STATISTICS %I.%I IS %L;', n.nspname, st.stxname, obj_description(st.oid, 'pg_statistic_ext')) ELSE '' END
    FROM relations r JOIN pg_statistic_ext st ON st.stxrelid = r.oid JOIN pg_namespace n ON n.oid = st.stxnamespace WHERE NOT r.unsupported
    UNION ALL
    SELECT r.oid, 80, '', 'ALTER TABLE ' || r.target || ' REPLICA IDENTITY ' ||
        CASE r.relreplident WHEN 'f' THEN 'FULL' WHEN 'n' THEN 'NOTHING'
        WHEN 'i' THEN 'USING INDEX ' || (SELECT quote_ident(ic.relname) FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid WHERE i.indrelid = r.oid AND i.indisreplident)
        ELSE 'DEFAULT' END || ';' ||
        CASE WHEN r.relrowsecurity THEN E'\nALTER TABLE ' || r.target || ' ENABLE ROW LEVEL SECURITY;' ELSE '' END ||
        CASE WHEN r.relforcerowsecurity THEN E'\nALTER TABLE ' || r.target || ' FORCE ROW LEVEL SECURITY;' ELSE '' END
    FROM relations r WHERE NOT r.unsupported AND r.relkind IN ('r', 'p')
    UNION ALL
    SELECT r.oid, 90, '', 'ALTER ' || r.object_type || ' ' || r.target || ' OWNER TO ' || quote_ident(pg_get_userbyid(r.relowner)) || ';'
    FROM relations r WHERE NOT r.unsupported
    UNION ALL
    SELECT s.table_oid, 91, s.target, 'ALTER SEQUENCE ' || s.target || ' OWNER TO ' || quote_ident(s.owner) || ';'
    FROM sequences s JOIN relations r ON r.oid = s.table_oid WHERE NOT r.unsupported
    UNION ALL
    SELECT r.oid, 95, '', 'REVOKE ALL ON ' || r.target || ' FROM PUBLIC, ' || quote_ident(pg_get_userbyid(r.relowner)) || ';'
    FROM relations r WHERE NOT r.unsupported AND r.relacl IS NOT NULL
    UNION ALL
    SELECT r.oid, 96, acl.grantee::text || acl.privilege_type,
        'GRANT ' || acl.privilege_type || ' ON TABLE ' || r.target || ' TO ' ||
        CASE WHEN acl.grantee = 0 THEN 'PUBLIC' ELSE quote_ident(pg_get_userbyid(acl.grantee)) END ||
        CASE WHEN acl.is_grantable THEN ' WITH GRANT OPTION' ELSE '' END || ';'
    FROM relations r CROSS JOIN LATERAL aclexplode(r.relacl) acl WHERE NOT r.unsupported
    UNION ALL
    SELECT r.oid, 96, a.attname || acl.grantee::text || acl.privilege_type,
        'GRANT ' || acl.privilege_type || ' (' || quote_ident(a.attname) || ') ON TABLE ' || r.target || ' TO ' ||
        CASE WHEN acl.grantee = 0 THEN 'PUBLIC' ELSE quote_ident(pg_get_userbyid(acl.grantee)) END ||
        CASE WHEN acl.is_grantable THEN ' WITH GRANT OPTION' ELSE '' END || ';'
    FROM relations r JOIN pg_attribute a ON a.attrelid = r.oid CROSS JOIN LATERAL aclexplode(a.attacl) acl
    WHERE NOT r.unsupported AND a.attnum > 0 AND NOT a.attisdropped
    UNION ALL
    SELECT s.table_oid, 97, s.target, 'REVOKE ALL ON SEQUENCE ' || s.target || ' FROM PUBLIC, ' || quote_ident(s.owner) || ';'
    FROM sequences s JOIN relations r ON r.oid = s.table_oid WHERE NOT r.unsupported AND s.relacl IS NOT NULL
    UNION ALL
    SELECT s.table_oid, 98, s.target || acl.grantee::text || acl.privilege_type,
        'GRANT ' || acl.privilege_type || ' ON SEQUENCE ' || s.target || ' TO ' ||
        CASE WHEN acl.grantee = 0 THEN 'PUBLIC' ELSE quote_ident(pg_get_userbyid(acl.grantee)) END ||
        CASE WHEN acl.is_grantable THEN ' WITH GRANT OPTION' ELSE '' END || ';'
    FROM sequences s JOIN relations r ON r.oid = s.table_oid CROSS JOIN LATERAL aclexplode(s.relacl) acl WHERE NOT r.unsupported
)
SELECT r.nspname::text, r.relname::text, string_agg(s.sql, E'\n' ORDER BY s.phase, s.sort_key) AS ddl
FROM relations r JOIN statements s ON s.oid = r.oid GROUP BY r.oid, r.nspname, r.relname
ORDER BY r.nspname, r.relname
