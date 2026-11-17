# FastDB PostgreSQL demo

Connect from FastDB with the PostgreSQL provider and these local fixture settings.

| Setting | Value |
| --- | --- |
| Host | `127.0.0.1` |
| Port | `5432` |
| Database | `fastdb_demo` |
| User | `postgres` |
| Password | The requested local fixture password |
| Unix socket / TLS / SSH | Off for this profile |
| Empty restore database | `fastdb_restore` |

`fastdb_reader` and `fastdb_writer` use the same fixture password. The reader can browse and query. The writer can edit table data and cannot administer objects it does not own. Both see tenant 1 by default in `lab.tenant_documents`. The superuser sees all three rows. Enable FastDB's **Read-only** profile option separately to test its own write protection.

| Schema | Tables | What to open |
| --- | ---: | --- |
| `shop` | 16 | `customers` (120), `orders` (500), `order_items` (1,500), `shipments`, inventory, supplier and employee junctions, self-referencing categories |
| `analytics` | 5 | `events` (9,000), monthly partitions and default partition, `monthly_sales` materialized view (3 rows) |
| `lab` | 12 | `notes`, keyless `raw_events` (15), `data_types`, `large_results` (250,000), writable rows, rules, RLS and empty import targets |
| `release_check`, `release_other` | 11 | Composite, self and cross-schema foreign keys, editable/empty tables, DDL scratch tables, views and another partition |
| `fastdb_meta` | 1 | Seed version marker |

Counts include partition roots and leaves. The clean fixture has 23 foreign key constraints. All business data is synthetic.

| FastDB flow | Try this |
| --- | --- |
| Navigator, Properties, DDL, ER diagram | Open `shop.orders` and `release_check.items`. Follow incoming/outgoing, composite and self references. Inspect `shop.products` generated `name_search` column and its comment, checks, partial/covering/GIN indexes and replayable DDL. Export the ER diagram as SVG. |
| Filters, group/sort, table search, history | Filter `shop.orders` by `customer_id=1`; expect 5 records. Search `lab.notes` for `needle_%`; expect the literal match in row 1. Compare SQL NULL and empty text in notes 4 and 3. Try history back/forward, Record/Text view and Copy. |
| Streaming, paging, grid and export | Run `SELECT * FROM lab.large_results ORDER BY id`. Expect 250,000 rows. Try fetch next, column resize, copy, export and Cancel on a running query. |
| Types and values | Open `lab.data_types`. Row 1 has Unicode, exact numeric values, JSON with repeated keys, JSONB, XML, arrays, ranges and 300,000-byte BLOB. Rows 2 and 3 distinguish empty and SQL NULL. Open `lab."Odd Table"` for quoted identifiers. Open `lab.raw_events` to check duplicate keyless rows. |
| Grid edits and transactions | Edit `lab.editable_rows`, use Save/Cancel, Auto/Manual, Commit and Rollback. `lab.audit_log` records committed changes. A negative `amount` should fail its CHECK and leave stored data unchanged. |
| SQL tools | Use a script on `shop.order_totals` for completion, snippets, named parameters, multi-statement execution, execution log and transaction log. `SELECT pg_sleep(20)` provides a cancel or timeout target. |
| Plans | From the SQL editor Explain `SELECT id,label,amount FROM lab.large_results WHERE bucket=7 AND amount BETWEEN 100 AND 102 ORDER BY amount`. Expect `large_results_bucket_amount_idx`. For partition pruning, explain `SELECT count(*) FROM analytics.events WHERE happened_on=DATE '2026-10-01'`. Expect only `events_2026_10`. |
| Schema drafts | Use `release_check.ddl_scratch` for column and table mutations. Verify SQL preview, Revert, Persist and metadata read-back. |
| Import, transfer, export | Import a CSV with `id,note` headers into `lab.csv_import_target`. Use `lab.transfer_target` for a table transfer. Try loaded CSV, SQL, JSON, HTML and Markdown exports, plus full CSV export where available. |
| Administration | Right-click the profile and choose **Administration…**. Run `SELECT lab.sum_values(VARIADIC ARRAY[1,2,3]::numeric[])` and expect `6`. Use `lab.ticket_numbers` for sequence restart, `editable_audit` for trigger state, `keep_rule_rows` for rules, and `tenant_isolation` for policies. Inspect partitions, `fastdb_analyst` membership and relation ACLs. Use disposable roles and partitions for create/drop operations. |
| Permission errors and RLS | With `fastdb_reader`, select `lab.tenant_documents`. Expect IDs 1 and 2. An INSERT into `lab.notes` should fail. Select `lab.ticket_numbers` in Administration, choose **Restart sequence**, then **Review SQL…** and **Confirm and execute**. Expect an ownership error and the SQL to remain in the editor. |
| Backup and maintenance | Back up `fastdb_demo`, then restore into `fastdb_restore`. Compare counts and data. Use Analyze, Vacuum and Reindex on the demo. Set **Client Directory** to the local native client directory below if automatic discovery fails. |

For Session Monitor, use two independent `psql` connections to `fastdb_demo` (client A and client B). Run `SELECT pg_advisory_lock(912010, 1)` in client A, then the same statement in client B. Open **Session Monitor…** and check the waiting and blocking PIDs. Cancel B's query, then release A's lock with `SELECT pg_advisory_unlock(912010, 1)` in client A. The linked release runbook also covers multi-session chains and termination.

- [PostgreSQL manual acceptance runbook](../0.2.0-postgres-manual-acceptance.md)
- [Backup and restore manual acceptance](../p2-02-manual-acceptance.md)
- [Administration workflow](../../guide/navigator-and-tables.md#postgresql-administration)

The fixture exercises PostgreSQL workflows against one server. Other providers and TLS/SSH require their own configurations.

The rootless systemd service uses the pinned official PostgreSQL 18 image and a named volume. It restarts on failure and starts with the user session. See the [Quadlet documentation](https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html) for user-service behavior.

The native PostgreSQL clients on this machine are in the following directory. Select it with **Client Directory** for backup, restore and maintenance.

```text
/home/thang-tran/Desktop/fastdb/target/postgres-demo-client/bin
```

```sh
systemctl --user status fastdb-postgres-demo.service
systemctl --user restart fastdb-postgres-demo.service
journalctl --user -u fastdb-postgres-demo.service -n 50
```

Seed once with `psql -f seed.sql` while connected to `fastdb_demo`, supplying `FASTDB_DEMO_PASSWORD` in the environment for the test roles. The version marker keeps reruns from overwriting edits. Recreating a container preserves the named volume. To repeat a restore, prepare another empty database so you can retain previous restore results.
