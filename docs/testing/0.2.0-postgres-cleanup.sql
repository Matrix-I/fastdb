-- Reset only the reserved acceptance schemas in the current disposable database.
-- CASCADE also removes dependent objects; never use these schemas for other work.
-- Close active test transactions before running; a lock timeout rolls back both drops.
BEGIN;
SET LOCAL lock_timeout = '3s';
SET LOCAL statement_timeout = '15s';
DROP SCHEMA IF EXISTS release_other CASCADE;
DROP SCHEMA IF EXISTS release_check CASCADE;
COMMIT;
