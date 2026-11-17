BEGIN;
SET LOCAL statement_timeout = '30s';

DO $fixture$
DECLARE
    schema_id integer;
    table_id integer;
    schema_name text;
BEGIN
    FOR schema_id IN 1..20 LOOP
        schema_name := 'perf_' || schema_id;
        EXECUTE format('CREATE SCHEMA %I', schema_name);
        FOR table_id IN 1..100 LOOP
            EXECUTE format(
                'CREATE TABLE %I.%I (id integer PRIMARY KEY, label text, amount numeric, happened_at timestamptz, payload bytea)',
                schema_name,
                'table_' || table_id
            );
        END LOOP;
    END LOOP;
END;
$fixture$;

ANALYZE pg_catalog.pg_class;
ANALYZE pg_catalog.pg_attribute;
ANALYZE pg_catalog.pg_constraint;
COMMIT;
