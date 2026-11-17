SELECT c.oid AS id,n.nspname::text AS schema,c.relname::text AS name,''::text AS parent,
 format('type: %s; start: %s; increment: %s; min: %s; max: %s; cache: %s; cycle: %s; owner: %s; last value: %s',
 format_type(s.seqtypid,NULL),s.seqstart,s.seqincrement,s.seqmin,s.seqmax,s.seqcache,s.seqcycle,
 pg_get_userbyid(c.relowner),pg_sequence_last_value(c.oid)) AS details,
 format('ALTER SEQUENCE %I.%I INCREMENT BY %s MINVALUE %s MAXVALUE %s START WITH %s CACHE %s %s;',
 n.nspname,c.relname,s.seqincrement,s.seqmin,s.seqmax,s.seqstart,s.seqcache,CASE WHEN s.seqcycle THEN 'CYCLE' ELSE 'NO CYCLE' END) AS source,
 NULL::text AS invocation FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_sequence s ON s.seqrelid=c.oid
 WHERE n.nspname !~ '^pg_(toast|temp)'
