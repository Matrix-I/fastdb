# FastDB PostgreSQL demo fixture

The user requested one stable local PostgreSQL server with the supplied test credentials and more than ten tables, both related and independent. Use rootless Podman with a persistent volume, loopback TCP, SCRAM and a user systemd service. Use the available port 5432. Keep one seeded database and a separate empty restore target in the same instance. No commit is requested.

- Deploy the pinned PostgreSQL image with the supplied test password in Podman secret storage. Verify real authenticated TCP, health and service restart.
- Apply the SQL in `docs/testing/postgres-demo/seed.sql` using `psql` in an isolated fixture database. Seed once in a transaction with a version marker and a lock that serializes concurrent setup. Reuse the release fixture and add commerce, analytics and type coverage.
- Verify table and FK counts, empty/keyless cases, large results, routines, roles and policy behavior. Run native plans for representative joins, indexed reads and partition pruning. Restart and confirm persisted counts. Record concrete FastDB smoke steps and expected values in the runbook.

## Verification

- The installed user service reported active and the container reported healthy after restart. PostgreSQL 18.7 answered on loopback TCP. The supplied password authenticated and an incorrect password was refused.
- The seed committed 45 tables, 23 foreign keys, 500 orders and 250,000 streaming rows. The seed rerun detected its version marker and preserved the data. After restarting the service, the version and streaming count persisted. The prepared restore database contained zero user relations.
- The reader saw RLS rows 1 and 2, was denied INSERT and received the native ownership error when attempting to restart `lab.ticket_numbers`. The variadic routine returned 6 and the materialized view held three monthly rows.
- `EXPLAIN (ANALYZE, BUFFERS)` used `large_results_bucket_amount_idx` for the two-row range in 0.035 ms (2 hits, 3 reads). The single-date query pruned to the October partition and returned 100 entries in 0.054 ms (34 hits). The commerce query returned five grouped rows in 0.254 ms (30 hits), used the customer index and scanned the 1,500-row line-item table once. The sort used 25 KiB and did not spill. No speedup is claimed from these individual executions.
- Native PostgreSQL 18 clients dumped and restored the fixture in a transaction on the empty target, then read back all 250,000 rows and rolled back. The target remained empty. The native clients are available in `target/postgres-demo-client/bin`.
- The FastDB config contains the saved demo profile with its keychain reference. The app's Linux credential backend successfully read back the matching secret. The config file is owner-only, and GUI acceptance is pending with the user.
- The Sol fixture reviewer found that the runbook stopped at Review SQL before native permission enforcement and was imprecise about the generated column comment. The runbook now includes Confirm and execute, the verified ownership error and the exact `name_search` column.
