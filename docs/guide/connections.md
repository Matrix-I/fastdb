# Connections

- **PostgreSQL** (`tokio-postgres`), **MySQL / MariaDB** (`mysql_async`) and **SQLite** (`rusqlite`, bundled). MariaDB has its own provider preset, saved identity and MySQL-compatible SQL handling.
- **Db2 for LUW**, **Oracle** and **SQL Server** support connections, SQL execution and streamed results through installed vendor ODBC drivers. The driver manager is bundled. See [provider setup and live tests](../PROVIDERS.md).
- TCP, or automatic local **Unix domain socket** discovery for PostgreSQL (`/tmp/.s.PGSQL.*`, `/var/run/postgresql`, `/run/postgresql`) and MySQL.
- Saved connections with create / edit / delete / test, plus quick presets per driver.
- **TLS for PostgreSQL and MySQL**: enable TLS in the connection dialog to require encryption and verify the server certificate and hostname. Leave the root CA path empty for system trust, or supply a PEM/DER root CA certificate for a private CA. TLS uses TCP even when Unix socket discovery is enabled. Existing connections default to TLS disabled.
- Saved connection passwords use the OS keychain (macOS Keychain / Linux Secret Service). Existing plaintext passwords migrate automatically after successful keychain storage; config files retain only credential references. Keychain/save failures are shown in the workspace with a retry action.

## Where settings are stored

For an isolated development profile, set `FASTDB_CONFIG_DIR=/tmp/fastdb-smoke` before launching. This keeps saved connections, scripts and update preferences separate from your normal profile.

Saved connections are written to `connections.json` in the user config directory (`~/Library/Application Support/com.fastdb.FastDB/` on macOS, `~/.config/fastdb/` on Linux), so they survive rebuilds and launches from Finder. A `.fastdb_config.json` left in the working directory by older builds is migrated on first launch and removed after a successful save. Passwords are stored under the `com.fastdb.credentials` keychain service, keyed by connection ID; JSON contains a `keychain_password` reference flag, never a new plaintext password. Migration retains the source file if secure storage fails.

An unlocked macOS Keychain or Linux Secret Service is required to save passwords. If a saved secret is missing or inaccessible, its reference is preserved and connecting requires re-entering the password or unlocking the keychain and restarting. Clearing a password or deleting a connection removes its stored secret; failed deletions are retried on the next save or startup. Failed saves show a workspace banner with **Retry saving connections** and block Quit until a successful retry. Ad-hoc signed macOS updates may prompt again for keychain access; cross-update access still needs release smoke testing.
