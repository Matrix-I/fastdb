# P2-02 manual acceptance note

Desktop acceptance is **deferred** at the user's request on 2026-11-16.
The roadmap checkbox tracks implementation and automated validation. Record
the desktop results here when the manual checks are run.

Record the tested commit, operating system, provider/client versions, tester
and date. Use disposable source and empty target databases, or SQLite files.
Launch the built `target/debug/fastdb` and use the Navigator context menus.

## Checklist

- [ ] For PostgreSQL, MySQL and SQLite, open **Backup Database…**, choose the
  native client directory and a new output filename, then run the backup.
  Verify progress, recent native output, successful status and the output file.
  Attempt an existing output filename and verify it is preserved.
- [ ] Run **Backup Table…** on each provider. Restore into an empty target and
  compare the selected schema, rows, NULL, empty text and binary values.
  Check PostgreSQL partition-child rows and SQLite indexes and AUTOINCREMENT.
- [ ] Run **Backup View…** and verify its definition with the dependencies it
  needs. For scoped PostgreSQL archives in a custom schema, enter the exact
  **Source schema for scoped archive** in the restore dialog.
- [ ] Restore full backups into separate empty PostgreSQL/MySQL databases.
  For MySQL, use a profile with privileges confined to the exact target schema.
  Check view, trigger, routine and event behavior under the restore account.
  Verify populated targets are rejected.
- [ ] Restore SQLite into a new file and an existing empty file. Reconnect
  and compare schema and data. Verify a populated destination is preserved.
- [ ] Cancel an active backup and confirm that no completed backup file is
  published. Exercise missing-client and invalid-archive errors.
- [ ] Run PostgreSQL/SQLite Vacuum, Analyze and Reindex, and MySQL Analyze
  Table and Optimize Table. Verify status and native output.
- [ ] Resize the dialog to a short window while a job runs. Verify the footer
  and **Cancel Job** remain reachable. Check target editing, confirmation reset,
  Close/Escape, and metadata refresh after restoring into a connected target.

## Known scope and precautions

Use trusted backup files and reserve restore targets for the operation.
PostgreSQL restores in one transaction. MySQL errors or cancellation can
leave DDL applied. MySQL generated definers become the restore account.
Server binary-log policy may require administrative approval of stored
program creation.

SQLite virtual tables require full-image backups. PKCS#12 client identities
and MySQL profiles combining SSH with TLS are unavailable for native tools.
MySQL through SSH remains unverified.

Automated verification and prior provider evidence are recorded in the
[implementation plan](../plans/2026-11-15-p2-02-backup-restore.md).

## Results

The Linux continuation exercised the packaged candidate through automated
desktop input. The macOS run is deferred.

| Case | Linux result |
| --- | --- |
| PostgreSQL scoped table archive | PASS. Restore into an empty database with the exact source schema preserved NULL, empty text and binary values. Only the selected table was restored. |
| PostgreSQL scoped partitioned archive and missing client | PASS. Backup Table captured both nonempty children. Restore preserved their boundaries, rows, keys and exact bytes. A missing `pg_dump` override displayed an error and published no file. [Evidence](2026-11-22-full-feature-acceptance-results.md#postgresql-partitioned-table-backup-and-restore) |
| PostgreSQL invalid archive and target confirmation | PASS. The invalid archive left the target empty. Editing the target cleared confirmation and disabled Run. |
| PostgreSQL full backup, existing output, populated target, cancellation and maintenance | See the packaged GUI observations in the acceptance report. |
| MySQL and SQLite desktop backup and restore | FAIL. The tested candidate omitted the native-tool entries. Independent automated native-client results are recorded separately. |
| PostgreSQL short running modal | PASS. At 1280 × 350, native output and Cancel Job remained visible. Cancel reaped the controlled child without leaving an output file. Escape closed the modal. [Evidence](2026-11-22-full-feature-acceptance-results.md#postgresql-backup-cancellation-in-a-short-window) |
| PostgreSQL scoped view with its base table | PASS. The archive contained the view and its dependency. Restore preserved the view definition and result rows. [Evidence](2026-11-22-full-feature-acceptance-results.md#postgresql-view-restore-and-connected-target) |
| PostgreSQL connected-target metadata refresh | BLOCKED by the restore guard. The connected target refused restore and stayed empty. After disconnect, restore succeeded and reconnect loaded the objects in Navigator. |

[Linux restore retest](2026-11-22-full-feature-acceptance-results.md#postgresql-scoped-restore-and-invalid-archive-retest)
and [native-tool evidence](2026-11-22-full-feature-acceptance-results.md#postgresql-native-tool-dialogs).
