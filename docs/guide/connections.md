# Connections

- **PostgreSQL** (`tokio-postgres`), **MySQL / MariaDB** (`mysql_async`) and **SQLite** (`rusqlite`, bundled). MariaDB has its own provider preset, saved identity and MySQL-compatible SQL handling.
- **Db2 for LUW**, **Oracle** and **SQL Server** support connections, SQL execution and streamed results through installed vendor ODBC drivers. The driver manager is bundled. See [provider setup and live tests](../PROVIDERS.md).
- TCP, or automatic local **Unix domain socket** discovery for PostgreSQL (`/tmp/.s.PGSQL.*`, `/var/run/postgresql`, `/run/postgresql`) and MySQL.
- Saved connections with create / edit / delete / test, plus quick presets per driver.
- **TLS for PostgreSQL and MySQL**: enable TLS in the connection dialog to require encryption and verify the server certificate and hostname. Leave the root CA path empty for system trust, or supply a PEM/DER root CA certificate for a private CA. TLS uses TCP even when Unix socket discovery is enabled. Existing connections default to TLS disabled.
- Saved connection passwords use the OS keychain (macOS Keychain / Linux Secret Service). Existing plaintext passwords migrate automatically after successful keychain storage; config files retain only credential references. Keychain/save failures are shown in the workspace with a retry action.

## Read-only connections

Enable **Read-only** for PostgreSQL, MySQL, MariaDB or SQLite to guard against
accidental writes. Reconnect to apply a profile change to an open connection.
The navigator displays the live connection's mode. Grid edits, schema changes
and transfer destinations are blocked. Read-only SQL uses provider safeguards.
Use database accounts with restricted grants for access control over privileged
server functions, extensions and external resources. SQLite read-only profiles
require an existing database file.

## Organize connections

Set **Navigator group** to show related connections under a shared heading.
Connections with an empty group appear first. Set **Navigator color** to an RGB
hex value such as `#d29922` to add a color marker beside a connection name.

## TLS client identity

When TLS is enabled on PostgreSQL, MySQL or MariaDB, you can supply a PKCS#12
archive (.p12 or .pfx) containing the client certificate and its private key.
Enter the archive password if required. FastDB stores the archive path in the
profile and the password in the OS keychain. Clear both fields to remove the
identity. Keep the server CA and hostname settings configured for the server you
intend to verify.

## PostgreSQL application name

Set **PostgreSQL application name** to identify FastDB sessions in
`pg_stat_activity` and server logs that include the application name. The label
can contain up to 63 printable ASCII bytes. It applies to new connections,
including isolated SQL tab sessions. An empty field leaves the server default.

## Connection timeout

Network profiles accept a timeout from 1 to 120 seconds. An empty field and
older profiles use 10 seconds. **Test** and **Connect** use this deadline for
the connection attempt, including SSH and database authentication. The initial
metadata load after connecting has a separate 15-second deadline.

## Connect through SSH

For PostgreSQL, MySQL, or MariaDB, keep the database address in the main
connection fields and enable **Connect through SSH**. Enter the jump host,
SSH username, and its trusted OpenSSH `SHA256:…` host key fingerprint. Get
that fingerprint from the administrator or another trusted source. Enter
either an SSH password or a private key path. With a private key path, the
secret field is the key's passphrase. **Test** uses the current form values
and checks the database through the tunnel.

SSH secrets use a separate OS keychain service. The saved connection file
contains the jump host settings and a credential reference. If TLS is enabled,
FastDB checks the database certificate against the database hostname.

## Where settings are stored

For an isolated development profile, set `FASTDB_CONFIG_DIR=/tmp/fastdb-smoke` before launching. This keeps saved connections, scripts and update preferences separate from your normal profile.

Saved connections are written to `connections.json` in the user config directory (`~/Library/Application Support/com.fastdb.FastDB/` on macOS, `~/.config/fastdb/` on Linux), so they survive rebuilds and launches from Finder. A `.fastdb_config.json` left in the working directory by older builds is migrated on first launch and removed after a successful save. Passwords are stored under the `com.fastdb.credentials` keychain service, keyed by connection ID; JSON contains a `keychain_password` reference flag, never a new plaintext password. Migration retains the source file if secure storage fails.

An unlocked macOS Keychain or Linux Secret Service is required to save passwords. If a saved secret is missing or inaccessible, its reference is preserved and connecting requires re-entering the password or unlocking the keychain and restarting. Clearing a password or deleting a connection removes its stored secret; failed deletions are retried on the next save or startup. Failed saves show a workspace banner with **Retry saving connections** and block Quit until a successful retry. Ad-hoc signed macOS updates may prompt again for keychain access; cross-update access still needs release smoke testing.
