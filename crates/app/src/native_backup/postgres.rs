use super::process::{self, BackupOutput, Outcome};
use super::tools::Tool;
use super::{BackupArtifact, Maintenance};
use anyhow::{Context, Result, ensure};
use db_wire::{ConnectionConfig, DbDriverKind, SshTunnel, sql::SqlDialect};
use platform_hal::socket::{DbKind, detect_local_unix_socket};
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::sync::watch;

const JOB_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const TARGET_CHECK: &str = include_str!("postgres_target.sql");

pub(crate) struct PgConnection {
    settings: ConnectionConfig,
    socket_dir: Option<PathBuf>,
    tunnel: Option<SshTunnel>,
    passfile: tempfile::NamedTempFile,
    ca: Option<PathBuf>,
}

impl PgConnection {
    pub(crate) async fn new(config: &ConnectionConfig) -> Result<Self> {
        ensure!(
            config.driver == DbDriverKind::Postgres,
            "Expected a PostgreSQL profile"
        );
        ensure!(
            config.tls_client_identity.is_none(),
            "PostgreSQL native tools require a profile without a PKCS#12 client identity"
        );
        ensure!(
            !config.host.is_empty() && !config.host.contains([',', '/', '\0']),
            "Expected a single PostgreSQL host"
        );
        ensure!(
            !config.user.is_empty() && config.port > 0,
            "PostgreSQL requires a username and port"
        );
        let ca = if config.tls_enabled {
            config
                .tls_ca_cert
                .as_deref()
                .map(|path| Path::new(path).canonicalize())
                .transpose()
                .context("Cannot locate the PostgreSQL TLS CA certificate")?
        } else {
            None
        };
        let mut passfile =
            tempfile::NamedTempFile::new().context("Cannot create PostgreSQL password file")?;
        if let Some(password) = &config.password {
            ensure!(
                !password.contains(['\r', '\n', '\0']),
                "The PostgreSQL password contains an unsupported character for native tools"
            );
            let escaped = password.replace('\\', "\\\\").replace(':', "\\:");
            writeln!(passfile, "*:*:*:*:{escaped}")
                .context("Cannot write PostgreSQL password file")?;
        }
        let tunnel = if let Some(settings) = config.ssh_tunnel.clone() {
            Some(
                tokio::time::timeout(
                    config.connect_timeout,
                    SshTunnel::start_forwarding(
                        settings,
                        config.host.clone(),
                        u32::from(config.port),
                    ),
                )
                .await
                .context("PostgreSQL native SSH connection timed out")??,
            )
        } else {
            None
        };
        let socket_dir = if tunnel.is_none() && config.use_unix_socket && !config.tls_enabled {
            detect_local_unix_socket(DbKind::Postgres, &config.host, config.port)
                .and_then(|socket| socket.parent().map(Path::to_path_buf))
        } else {
            None
        };
        Ok(Self {
            settings: config.clone(),
            socket_dir,
            tunnel,
            passfile,
            ca,
        })
    }

    fn command(&self, executable: &Path, database: &str) -> Result<Command> {
        ensure!(
            !database.is_empty() && !database.contains('\0'),
            "Expected a PostgreSQL database name"
        );
        let mut command = Command::new(executable);
        command
            .env_clear()
            .env(
                "PGPORT",
                self.tunnel
                    .as_ref()
                    .map_or(self.settings.port, |tunnel| tunnel.local_bound_port)
                    .to_string(),
            )
            .env("PGUSER", &self.settings.user)
            .env("PGPASSFILE", self.passfile.path())
            .env(
                "PGCONNECT_TIMEOUT",
                self.settings.connect_timeout.as_secs().max(2).to_string(),
            )
            .env("PGCLIENTENCODING", "UTF8")
            .env(
                "PGAPPNAME",
                self.settings
                    .application_name
                    .as_deref()
                    .unwrap_or("FastDB Native Tools"),
            )
            .env("PGGSSENCMODE", "disable")
            .env(
                "PGSSLMODE",
                if self.settings.tls_enabled {
                    "verify-full"
                } else {
                    "disable"
                },
            );
        if let Some(directory) = &self.socket_dir {
            command.env("PGHOST", directory);
        } else {
            command.env("PGHOST", &self.settings.host);
        }
        if self.tunnel.is_some() {
            // Keep PGHOST for certificate verification while forwarding the socket.
            command.env("PGHOSTADDR", "127.0.0.1");
        }
        if self.settings.tls_enabled {
            command.env(
                "PGSSLROOTCERT",
                self.ca.as_deref().unwrap_or(Path::new("system")),
            );
            // Ignore an unrelated default client identity from the user's home directory.
            command.env(
                "PGSSLCERT",
                self.passfile.path().with_extension("no-client-cert"),
            );
        }
        if self.settings.read_only {
            command.env("PGOPTIONS", "-c default_transaction_read_only=on");
        }
        // Native tools reinterpret values containing '=' or a URI prefix as
        // connection strings. Force a single escaped dbname parameter.
        let literal = database.replace('\\', "\\\\").replace('\'', "\\'");
        command
            .arg(format!("--dbname=dbname='{literal}'"))
            .arg("--no-password");
        Ok(command)
    }

    pub(crate) async fn backup(
        &self,
        directory: Option<&Path>,
        destination: &Path,
        scope: &[(String, String)],
        cancel: watch::Receiver<bool>,
        logs: watch::Sender<String>,
    ) -> Result<Outcome> {
        let executable = Tool::PgDump.resolve(directory)?;
        let artifact = BackupArtifact::stage(destination)?;
        let command = backup_command(
            self.command(&executable, &self.settings.database)?,
            &artifact,
            scope,
        )?;
        logs.send_replace("Backing up the PostgreSQL database…".to_owned());
        process::run(
            command,
            None,
            Some(BackupOutput::NativeFile(artifact)),
            cancel,
            logs,
            JOB_TIMEOUT,
        )
        .await
    }

    pub(crate) async fn maintain(
        &self,
        directory: Option<&Path>,
        operation: Maintenance,
        cancel: watch::Receiver<bool>,
        logs: watch::Sender<String>,
    ) -> Result<Outcome> {
        ensure!(
            !self.settings.read_only,
            "PostgreSQL maintenance requires a writable profile"
        );
        let tool = match operation {
            Maintenance::Vacuum | Maintenance::Analyze => Tool::VacuumDb,
            Maintenance::Reindex => Tool::ReindexDb,
        };
        let mut command = self.command(&tool.resolve(directory)?, &self.settings.database)?;
        command.arg("--verbose");
        if matches!(operation, Maintenance::Analyze) {
            command.arg("--analyze-only");
        }
        logs.send_replace(format!("Running PostgreSQL {}…", tool.name()));
        process::run(command, None, None, cancel, logs, JOB_TIMEOUT).await
    }

    pub(crate) async fn restore(
        &self,
        directory: Option<&Path>,
        archive: &Path,
        database: &str,
        cancel: watch::Receiver<bool>,
        logs: watch::Sender<String>,
    ) -> Result<Outcome> {
        self.restore_with_schema(directory, archive, database, None, cancel, logs)
            .await
    }

    pub(crate) async fn restore_with_schema(
        &self,
        directory: Option<&Path>,
        archive: &Path,
        database: &str,
        schema: Option<&str>,
        cancel: watch::Receiver<bool>,
        logs: watch::Sender<String>,
    ) -> Result<Outcome> {
        ensure!(
            !self.settings.read_only,
            "PostgreSQL restore requires a writable profile"
        );
        ensure!(
            database != self.settings.database && !matches!(database, "template0" | "template1"),
            "Choose a separate PostgreSQL restore database"
        );
        let archive = archive
            .canonicalize()
            .context("Cannot locate the PostgreSQL archive")?;
        ensure!(archive.is_file(), "Select a PostgreSQL custom archive file");
        let mut magic = [0; 5];
        std::fs::File::open(&archive)?.read_exact(&mut magic)?;
        ensure!(
            &magic == b"PGDMP",
            "Select a PostgreSQL custom archive file"
        );

        let scratch =
            tempfile::tempdir().context("Cannot create PostgreSQL restore staging directory")?;
        let script = scratch.path().join("restore.sql");
        let staged_script = BackupArtifact::stage(&script)?;
        let mut unpack = Command::new(Tool::PgRestore.resolve(directory)?);
        unpack
            .env_clear()
            .args(["--file=-", "--no-owner", "--no-privileges", "--"])
            .arg(&archive);
        logs.send_replace("Preparing the PostgreSQL restore script…".to_owned());
        let outcome = process::run(
            unpack,
            None,
            Some(BackupOutput::Stdout(staged_script)),
            cancel.clone(),
            logs.clone(),
            JOB_TIMEOUT,
        )
        .await?;
        if outcome == Outcome::Canceled {
            return Ok(Outcome::Canceled);
        }

        let mut restore = self.command(&Tool::Psql.resolve(directory)?, database)?;
        let mut guard = restore_guard();
        if let Some(schema) = schema {
            ensure!(
                !schema.is_empty() && !schema.contains('\0'),
                "Enter the source schema name"
            );
            ensure!(
                schema != "information_schema" && !schema.starts_with("pg_"),
                "Choose a user schema"
            );
            guard.push_str(&format!(
                " CREATE SCHEMA IF NOT EXISTS {};",
                SqlDialect(DbDriverKind::Postgres).quote_ident(schema)
            ));
        }
        restore.env("PGAPPNAME", "FastDB Native Restore");
        restore
            .args([
                "-X",
                "--single-transaction",
                "--set=ON_ERROR_STOP=1",
                "--command",
            ])
            .arg(guard)
            .arg("--file")
            .arg(script);
        logs.send_replace(
            "Checking and restoring the PostgreSQL database in one transaction…".to_owned(),
        );
        process::run(restore, None, None, cancel, logs, JOB_TIMEOUT).await
    }
}

fn restore_guard() -> String {
    let check = TARGET_CHECK.trim().trim_end_matches(';');
    // Advisory locks are scoped to a database. Use a stable key shared by
    // FastDB clients and retain the lock until the restore transaction ends.
    format!(
        r#"DO $fastdb_restore$
        DECLARE target_state text;
        BEGIN
            IF NOT pg_catalog.pg_try_advisory_xact_lock(
                pg_catalog.hashtextextended('FastDB native restore', 0)
            ) THEN
                RAISE EXCEPTION 'Another FastDB restore is running for this database';
            END IF;
            target_state := ({check});
            IF target_state = 'busy' THEN
                RAISE EXCEPTION 'Disconnect other sessions from the PostgreSQL restore database';
            ELSIF target_state <> 'empty' THEN
                RAISE EXCEPTION 'PostgreSQL restore requires a separate empty database';
            END IF;
        END
        $fastdb_restore$;"#
    )
}

fn backup_command(
    mut command: Command,
    artifact: &BackupArtifact,
    scope: &[(String, String)],
) -> Result<Command> {
    command.args(["--format=custom", "--verbose", "--lock-wait-timeout=5s"]);
    command.arg("--file").arg(artifact.path());
    if !scope.is_empty() {
        command.arg("--strict-names");
    }
    for (schema, table) in scope {
        ensure!(
            !schema.is_empty()
                && !table.is_empty()
                && !schema.contains('\0')
                && !table.contains('\0'),
            "Invalid PostgreSQL relation name"
        );
        let pattern = SqlDialect(DbDriverKind::Postgres).qualified_table(schema, table);
        command.arg(format!("--table-and-children={pattern}"));
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[test]
    fn live_backup_restore_scope_and_maintenance() -> Result<()> {
        let Some(port) = std::env::var_os("FASTDB_TEST_P202_PG_PORT") else {
            return Ok(());
        };
        let directory = std::env::var("FASTDB_TEST_P202_PG_BIN")?;
        let directory = Path::new(&directory);
        let ca = std::env::var("FASTDB_TEST_P202_PG_TLS_CA").ok();
        let ssh_tunnel = std::env::var("FASTDB_TEST_P202_SSH_PORT")
            .ok()
            .map(|port| {
                Ok::<_, anyhow::Error>(db_wire::SshTunnelConfig {
                    host: "127.0.0.1".into(),
                    port: port.parse()?,
                    user: std::env::var("FASTDB_TEST_P202_SSH_USER")?,
                    key_path: Some(std::env::var("FASTDB_TEST_P202_SSH_KEY")?),
                    password: None,
                    host_key_fingerprint: std::env::var("FASTDB_TEST_P202_SSH_FINGERPRINT")?,
                })
            })
            .transpose()?;
        let config = ConnectionConfig {
            database: "p202_source".into(),
            host: if ca.is_some() {
                "localhost"
            } else {
                "127.0.0.1"
            }
            .into(),
            port: port.to_string_lossy().parse()?,
            user: "p202_fixture".into(),
            password: Some("p202:fixture\\pass".into()),
            use_unix_socket: false,
            tls_enabled: ca.is_some(),
            tls_ca_cert: ca,
            ssh_tunnel,
            ..Default::default()
        };
        let files = tempfile::tempdir()?;
        let full = files.path().join("full archive");
        let scoped = files.path().join("scoped archive");
        let partitioned = files.path().join("partitioned archive");
        let (_cancel, receiver) = watch::channel(false);
        let (logs, details) = watch::channel(String::new());
        db_wire::get_runtime().block_on(async {
            let connection = PgConnection::new(&config).await?;
            let mut setup = connection.command(&Tool::Psql.resolve(Some(directory))?, "p202_source")?;
            setup.args(["-X", "--set=ON_ERROR_STOP=1", "-c",
                "CREATE SCHEMA \"chosen \"\"schema\"; CREATE TABLE \"chosen \"\"schema\".fixture(id INT PRIMARY KEY); INSERT INTO \"chosen \"\"schema\".fixture VALUES(37)"]);
            ensure!(setup.output()?.status.success());
            let namespaced = files.path().join("namespaced.dump");
            assert_eq!(
                connection.backup(
                    Some(directory), &namespaced, &[("chosen \"schema".into(), "fixture".into())],
                    receiver.clone(), logs.clone(),
                ).await?,
                Outcome::Completed
            );
            assert_eq!(
                connection.restore_with_schema(
                    Some(directory), &namespaced, "p202_schema", Some("chosen \"schema"),
                    receiver.clone(), logs.clone(),
                ).await.with_context(|| details.borrow().clone())?,
                Outcome::Completed
            );
            let mut check = connection.command(&Tool::Psql.resolve(Some(directory))?, "p202_schema")?;
            check.args(["-XAt", "-c", "SELECT id FROM \"chosen \"\"schema\".fixture"]);
            let result = check.output()?;
            ensure!(result.status.success());
            assert_eq!(result.stdout, b"37\n");
            if config.tls_enabled {
                let mut tls = connection.command(&Tool::Psql.resolve(Some(directory))?, "p202_source")?;
                tls.args(["-XAt", "-c", "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()"]);
                let result = tls.output()?;
                ensure!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
                assert_eq!(result.stdout, b"t\n");
            }
            for (destination, scope) in [
                (&full, vec![]),
                (&scoped, vec![("public".into(), "-special.*".into())]),
                (&partitioned, vec![("public".into(), "partitioned".into())]),
            ] {
                let result = connection
                    .backup(
                        Some(directory),
                        destination,
                        &scope,
                        receiver.clone(),
                        logs.clone(),
                    )
                    .await
                    .with_context(|| details.borrow().clone())?;
                assert_eq!(result, Outcome::Completed);
            }
            for operation in [
                Maintenance::Vacuum,
                Maintenance::Analyze,
                Maintenance::Reindex,
            ] {
                let result = connection
                    .maintain(Some(directory), operation, receiver.clone(), logs.clone())
                    .await
                    .with_context(|| details.borrow().clone())?;
                assert_eq!(result, Outcome::Completed);
            }
            for (archive, database) in [(&full, "p202_restore"), (&scoped, "p202_scoped")] {
                let outcome = connection
                    .restore(
                        Some(directory),
                        archive,
                        database,
                        receiver.clone(),
                        logs.clone(),
                    )
                    .await
                    .with_context(|| details.borrow().clone())?;
                assert_eq!(outcome, Outcome::Completed);
                let mut query =
                    connection.command(&Tool::Psql.resolve(Some(directory))?, database)?;
                query.args(["-XAt", "-c", r#"SELECT id FROM "-special.*""#]);
                let result = query.output()?;
                ensure!(result.status.success());
                assert_eq!(result.stdout, b"11\n");
            }
            assert_eq!(
                connection
                    .restore(
                        Some(directory),
                        &partitioned,
                        "p202_partitioned",
                        receiver.clone(),
                        logs.clone(),
                    )
                    .await
                    .with_context(|| details.borrow().clone())?,
                Outcome::Completed
            );
            let mut query =
                connection.command(&Tool::Psql.resolve(Some(directory))?, "p202_partitioned")?;
            query.args(["-XAt", "-c", "SELECT id FROM partitioned ORDER BY id"]);
            let result = query.output()?;
            ensure!(result.status.success());
            assert_eq!(result.stdout, b"12\n24\n");
            let (left_log, _left_details) = watch::channel(String::new());
            let (right_log, _right_details) = watch::channel(String::new());
            let (left, right) = tokio::join!(
                connection.restore(
                    Some(directory),
                    &scoped,
                    "p202_concurrent",
                    receiver.clone(),
                    left_log,
                ),
                connection.restore(
                    Some(directory),
                    &partitioned,
                    "p202_concurrent",
                    receiver.clone(),
                    right_log,
                ),
            );
            assert_eq!(
                [left, right]
                    .into_iter()
                    .filter(|result| matches!(result, Ok(Outcome::Completed)))
                    .count(),
                1,
                "exactly one restore may apply to the same empty database"
            );
            let mut session =
                connection.command(&Tool::Psql.resolve(Some(directory))?, "p202_busy")?;
            session.arg("-XAt");
            let mut session = tokio::process::Command::from(session)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;
            let mut input = session.stdin.take().context("Missing fixture stdin")?;
            input.write_all(b"SELECT 'ready';\n").await?;
            let mut output =
                BufReader::new(session.stdout.take().context("Missing fixture stdout")?);
            let mut ready = String::new();
            tokio::time::timeout(Duration::from_secs(5), output.read_line(&mut ready)).await??;
            assert_eq!(ready, "ready\n");
            let rejected = connection
                .restore(
                    Some(directory),
                    &full,
                    "p202_busy",
                    receiver.clone(),
                    logs.clone(),
                )
                .await;
            assert!(rejected.is_err());
            assert!(details.borrow().contains("Disconnect other sessions"));
            drop(input);
            ensure!(
                tokio::time::timeout(Duration::from_secs(5), session.wait())
                    .await??
                    .success()
            );
            let rejected = connection
                .restore(
                    Some(directory),
                    &full,
                    "p202_restore",
                    receiver.clone(),
                    logs.clone(),
                )
                .await;
            assert!(rejected.is_err());
            assert!(details.borrow().contains("empty database"));
            let rejected = connection
                .restore(
                    Some(directory),
                    &full,
                    "p202_populated",
                    receiver.clone(),
                    logs.clone(),
                )
                .await;
            assert!(rejected.is_err());
            assert!(details.borrow().contains("empty database"));
            Ok::<_, anyhow::Error>(())
        })?;
        let pg_restore = Tool::PgRestore.resolve(Some(directory))?;
        for (archive, includes_other) in [(full, true), (scoped, false)] {
            let result = Command::new(&pg_restore)
                .arg("--list")
                .arg(archive)
                .output()?;
            ensure!(result.status.success());
            let inventory = String::from_utf8(result.stdout)?;
            assert!(inventory.contains("TABLE DATA public -special.* "));
            assert_eq!(
                inventory.contains("TABLE DATA public other "),
                includes_other
            );
        }
        Ok(())
    }

    #[test]
    fn builds_a_literal_database_name_and_preserves_pgpass_escaping() -> Result<()> {
        let config = ConnectionConfig {
            database: "dbname=wrong ' db\\x".into(),
            password: Some("fixture:secret\\x".into()),
            use_unix_socket: false,
            ..Default::default()
        };
        let connection = db_wire::get_runtime().block_on(PgConnection::new(&config))?;
        let command = connection.command(Path::new("/fixture/pg_dump"), &config.database)?;
        assert!(
            command
                .get_args()
                .any(|arg| arg == "--dbname=dbname='dbname=wrong \\' db\\\\x'")
        );
        let settings: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert!(settings.keys().all(|key| *key != "PGPASSWORD"));
        let passfile = settings[std::ffi::OsStr::new("PGPASSFILE")].unwrap();
        assert_eq!(
            std::fs::read_to_string(passfile)?,
            "*:*:*:*:fixture\\:secret\\\\x\n"
        );
        Ok(())
    }

    #[test]
    fn selected_table_uses_one_literal_pattern_argument() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let stage = BackupArtifact::stage(&directory.path().join("backup"))?;
        let scope = [("Other.*".to_owned(), "-n \" quotes . *".to_owned())];
        let command = backup_command(Command::new("/fixture/pg_dump"), &stage, &scope)?;
        assert!(
            command
                .get_args()
                .any(|argument| argument
                    == "--table-and-children=\"Other.*\".\"-n \"\" quotes . *\"")
        );
        assert!(command.get_args().any(|arg| arg == "--strict-names"));
        Ok(())
    }
}
