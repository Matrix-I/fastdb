# Local provider servers

`compose.yml` defines disposable Oracle Free, SQL Server Developer and Db2
Community servers. Ports bind only to localhost. Use one profile at a time if
memory or disk space is limited. SQL Server and Db2 use x86_64 images; ARM hosts
need a working x86_64 emulator. Db2 needs privileged containers inside the Linux
VM. Do not attach production data or reuse production credentials.

On Apple Silicon, the SQL Server image tested here crashed under QEMU and ran
successfully with Podman's Rosetta support. Use a Linux VM with at least 6 GiB
of RAM and run the servers sequentially. Oracle needed a 3 GiB container limit;
with 2 GiB the server started but client logins stalled. The test process must
match its vendor driver's architecture: Oracle has an ARM64 client, while the
SQL Server and Db2 test clients used x86_64 binaries.

Set disposable passwords for all three variables (Compose interpolates all
services, even when only one profile is active):

```bash
export FASTDB_ORACLE_PASSWORD='FastDB_Oracle_Test_42'
export FASTDB_SQLSERVER_PASSWORD='FastDB_SqlServer_Test_42'
export FASTDB_DB2_PASSWORD='FastDB_Db2_Test_42'
```

After accepting the corresponding vendor terms for local testing, set
`FASTDB_ACCEPT_MSSQL_EULA=Y` and `FASTDB_ACCEPT_DB2_LICENSE=accept`. Defaults do
not accept these terms automatically.

```bash
docker compose -f ci/provider-containers/compose.yml --profile oracle up -d
# Wait for the Oracle healthcheck, then use localhost:11521 / FREEPDB1 / fastdb.
docker compose -f ci/provider-containers/compose.yml --profile oracle stop

docker compose -f ci/provider-containers/compose.yml --profile sqlserver up -d
# Wait for "ready for client connections", then use localhost:11433 / master / sa.
docker compose -f ci/provider-containers/compose.yml --profile sqlserver stop

docker compose -f ci/provider-containers/compose.yml --profile db2 up -d
# Wait for Db2 setup to finish, then use localhost:55000 / FASTDB / db2inst1.
```

Run the FastDB integration tests with the matching native ODBC client installed
(see `docs/PROVIDERS.md`). For example, against Oracle:

```bash
FASTDB_TEST_ORACLE_HOST=localhost FASTDB_TEST_ORACLE_PORT=11521 \
FASTDB_TEST_ORACLE_DATABASE=FREEPDB1 FASTDB_TEST_ORACLE_USER=fastdb \
FASTDB_TEST_ORACLE_PASSWORD="$FASTDB_ORACLE_PASSWORD" \
FASTDB_TEST_PROVIDER_WRITES=1 \
cargo test -p db-wire --test provider_test oracle_connects_and_runs_sql -- --nocapture
```

The write checks create/drop uniquely named tables and verify SELECT, INSERT,
UPDATE, DELETE and transaction rollback. Use SQLSERVER or DB2 instead of ORACLE
for the other providers' variables. A test runner in a container must use the
server container's network name and internal port, rather than localhost.

If the Db2 image reports `db2: command not found` during setup, do not treat its
`Setup has completed` message as proof that the database exists. For the
nonpersistent home configured here, explicitly load the instance profile and
finish setup (run `create db` only if FASTDB has not already been created):

```bash
docker compose -f ci/provider-containers/compose.yml exec db2 \
  su - db2inst1 -c '. /home/db2inst1/sqllib/db2profile; db2set DB2COMM=TCPIP; db2 update dbm cfg using SVCENAME 50000; db2stop force; db2start; db2 create db FASTDB using codeset utf-8 territory us collate using identity'
```

Install the complete IBM Data Server Driver for ODBC and CLI in the test runner.
Copying only `lib64` from the server image leaves out client resources and failed
ODBC environment initialization in this test environment.

Remove only this test project's containers and disposable volumes when finished:

```bash
docker compose -f ci/provider-containers/compose.yml --profile oracle --profile sqlserver --profile db2 down -v
```
