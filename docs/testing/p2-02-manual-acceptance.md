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

Pending. Record PASS, FAIL or BLOCKED for each case, with reproduction details
for failures and without credentials or sensitive connection information.
