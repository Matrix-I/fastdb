use super::process::{self, BackupOutput, Outcome};
use super::tools::Tool;
use super::{BackupArtifact, Maintenance};
mod portable;
use anyhow::{Context, Result, ensure};
use db_wire::{ConnectionConfig, DbDriverKind, SshTunnel, sql::SqlDialect};
use platform_hal::socket::{DbKind, detect_local_unix_socket};
use portable::portable_dump;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;

const JOB_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const TARGET_POPULATED: &str = "
    EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = DATABASE())
    OR EXISTS (SELECT 1 FROM information_schema.routines WHERE routine_schema = DATABASE())
    OR EXISTS (SELECT 1 FROM information_schema.events WHERE event_schema = DATABASE())";
const GRANT_CHECK: &str =
    "SELECT @@GLOBAL.partial_revokes; SHOW GRANTS; SELECT 'fastdb_grants_end';";

pub(crate) struct MysqlConnection {
    config: ConnectionConfig,
    socket: Option<PathBuf>,
    tunnel: Option<SshTunnel>,
    options: tempfile::NamedTempFile,
    ca: Option<PathBuf>,
}

impl MysqlConnection {
    pub(crate) async fn new(config: &ConnectionConfig) -> Result<Self> {
        ensure!(
            config.driver == DbDriverKind::MySql,
            "Expected a MySQL profile"
        );
        ensure!(
            !config.host.is_empty() && !config.user.is_empty() && config.port > 0,
            "MySQL requires a host, username and port"
        );
        ensure!(
            config.tls_client_identity.is_none(),
            "MySQL native tools require a profile without a PKCS#12 client identity"
        );
        ensure!(
            !(config.tls_enabled && config.ssh_tunnel.is_some()),
            "MySQL native tools cannot verify the TLS hostname through this SSH profile"
        );
        let ca = if config.tls_enabled {
            config
                .tls_ca_cert
                .as_deref()
                .map(|path| Path::new(path).canonicalize())
                .transpose()
                .context("Cannot locate the MySQL TLS CA certificate")?
        } else {
            None
        };
        let mut options =
            tempfile::NamedTempFile::new().context("Cannot create MySQL options file")?;
        writeln!(options, "[client]")?;
        if let Some(password) = &config.password {
            ensure!(
                !password.contains('\0'),
                "MySQL password contains an unsupported character for native tools"
            );
            let escaped = password
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\r', "\\r")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            writeln!(options, "password=\"{escaped}\"")?;
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
                .context("MySQL native SSH connection timed out")??,
            )
        } else {
            None
        };
        let socket = if tunnel.is_none() && config.use_unix_socket && !config.tls_enabled {
            detect_local_unix_socket(DbKind::MySql, &config.host, config.port)
        } else {
            None
        };
        Ok(Self {
            config: config.clone(),
            socket,
            tunnel,
            options,
            ca,
        })
    }

    fn command(&self, executable: &Path) -> Command {
        let mut command = Command::new(executable);
        let mut options = std::ffi::OsString::from("--defaults-file=");
        options.push(self.options.path());
        command
            .env_clear()
            .arg(options)
            .arg("--no-login-paths")
            .arg(format!("--user={}", self.config.user))
            .arg("--default-character-set=utf8mb4")
            .arg(if self.config.tls_enabled {
                "--ssl-mode=VERIFY_IDENTITY"
            } else {
                "--ssl-mode=DISABLED"
            });
        if let Some(ca) = &self.ca {
            let mut option = std::ffi::OsString::from("--ssl-ca=");
            option.push(ca);
            command.arg(option);
        }
        if let Some(socket) = &self.socket {
            let mut option = std::ffi::OsString::from("--socket=");
            option.push(socket);
            command.arg("--protocol=SOCKET").arg(option);
        } else {
            let (host, port) = self
                .tunnel
                .as_ref()
                .map_or((self.config.host.as_str(), self.config.port), |tunnel| {
                    ("127.0.0.1", tunnel.local_bound_port)
                });
            command
                .arg("--protocol=TCP")
                .arg(format!("--host={host}"))
                .arg(format!("--port={port}"));
            if !self.config.tls_enabled
                && (self.tunnel.is_some()
                    || host.eq_ignore_ascii_case("localhost")
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback()))
            {
                command.arg("--get-server-public-key");
            }
        }
        command
    }

    pub(crate) async fn backup(
        &self,
        directory: Option<&Path>,
        destination: &Path,
        scope: &[(String, String)],
        cancel: watch::Receiver<bool>,
        logs: watch::Sender<String>,
    ) -> Result<Outcome> {
        let executable = Tool::MysqlDump.resolve(directory)?;
        let stage = BackupArtifact::stage(destination)?;
        let mut command = self.command(&executable);
        dump_args(&mut command, &self.config.database, scope)?;
        logs.send_replace("Backing up the MySQL database…".to_owned());
        process::run(
            command,
            None,
            Some(BackupOutput::Stdout(stage)),
            cancel,
            logs,
            JOB_TIMEOUT,
        )
        .await
    }

    pub(crate) async fn restore(
        &self,
        directory: Option<&Path>,
        archive: &Path,
        database: &str,
        mut cancel: watch::Receiver<bool>,
        logs: watch::Sender<String>,
    ) -> Result<Outcome> {
        ensure!(
            !self.config.read_only,
            "MySQL restore requires a writable profile"
        );
        ensure!(
            !["sys", "mysql", "information_schema", "performance_schema"]
                .iter()
                .any(|reserved| database.eq_ignore_ascii_case(reserved)),
            "Choose a separate MySQL restore database"
        );
        ensure!(archive.is_file(), "Select a regular MySQL SQL dump file");
        let file = File::open(archive).context("Cannot open the MySQL SQL dump")?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.len() > 0,
            "Select a nonempty MySQL SQL dump"
        );

        let mut check = self.client(directory, database)?;
        check.args(["--raw", "--execute", GRANT_CHECK]);
        let (report, result) = watch::channel(String::new());
        logs.send_replace(
            "Checking that the MySQL restore account is confined to the target…".to_owned(),
        );
        let outcome = process::run(
            check,
            None,
            None,
            cancel.clone(),
            report,
            Duration::from_secs(30),
        )
        .await;
        if outcome? == Outcome::Canceled {
            return Ok(Outcome::Canceled);
        }
        ensure!(
            restore_grants_allowed(&result.borrow(), database),
            "MySQL restore requires an account with privileges confined to the exact target database"
        );

        logs.send_replace("Preparing the MySQL restore input…".to_owned());
        let staged = tokio::select! {
            biased;
            _ = async { let _ = cancel.wait_for(|requested| *requested).await; } => {
                return Ok(Outcome::Canceled);
            }
            result = tokio::time::timeout(JOB_TIMEOUT, stage_restore(file)) => {
                result.context("Preparing the MySQL restore input timed out")??
            }
        };
        let mut command = self.client(directory, database)?;
        command.arg("--binary-mode");
        logs.send_replace(
            "Checking and restoring MySQL SQL. Cancellation or failure can leave changes applied."
                .to_owned(),
        );
        process::run(command, Some(staged), None, cancel, logs, JOB_TIMEOUT).await
    }

    pub(crate) async fn maintain(
        &self,
        directory: Option<&Path>,
        operation: Maintenance,
        table: &str,
        cancel: watch::Receiver<bool>,
        logs: watch::Sender<String>,
    ) -> Result<Outcome> {
        ensure!(
            !self.config.read_only,
            "MySQL maintenance requires a writable profile"
        );
        ensure!(
            !table.is_empty() && !table.contains('\0'),
            "Choose a MySQL table"
        );
        let database = &self.config.database;
        let operation = match operation {
            Maintenance::Analyze => "ANALYZE",
            Maintenance::Vacuum | Maintenance::Reindex => "OPTIMIZE",
        };
        let relation = SqlDialect(DbDriverKind::MySql).qualified_table(database, table);
        let mut command = self.client(directory, database)?;
        command
            .arg("--execute")
            .arg(format!("{operation} TABLE {relation}"));
        logs.send_replace(format!("Running MySQL {operation} TABLE…"));
        let (report, result) = watch::channel(String::new());
        let outcome = process::run(command, None, None, cancel, report, JOB_TIMEOUT).await;
        let snapshot = result.borrow().clone();
        if !snapshot.is_empty() {
            logs.send_replace(snapshot.clone());
        }
        let outcome = outcome?;
        if outcome == Outcome::Completed {
            ensure!(
                snapshot.lines().any(|line| {
                    let columns: Vec<_> = line.split('\t').collect();
                    matches!(columns.as_slice(), [_, _, "status", "OK"])
                }),
                "MySQL {operation} did not report success"
            );
        }
        Ok(outcome)
    }

    fn client(&self, directory: Option<&Path>, database: &str) -> Result<Command> {
        ensure!(
            !database.is_empty() && !database.contains('\0'),
            "Choose a MySQL database"
        );
        let mut command = self.command(&Tool::Mysql.resolve(directory)?);
        command
            .args([
                "--batch",
                "--skip-column-names",
                "--local-infile=0",
                "--skip-reconnect",
            ])
            .arg(format!(
                "--connect-timeout={}",
                self.config.connect_timeout.as_secs().max(1)
            ))
            .arg(format!("--database={database}"));
        Ok(command)
    }
}

fn restore_grants_allowed(report: &str, database: &str) -> bool {
    if report.len() >= process::LOG_LIMIT - 3 {
        return false;
    }
    let mut lines = report.lines();
    let grant_pattern = match lines.next() {
        Some("1") => database.to_owned(),
        Some("0") => database.replace('_', "\\_").replace('%', "\\%"),
        _ => return false,
    };
    if lines.next_back() != Some("fastdb_grants_end") {
        return false;
    }
    let quoted = SqlDialect(DbDriverKind::MySql).quote_ident(&grant_pattern);
    let target = format!("GRANT ALL PRIVILEGES ON {quoted}.* TO ");
    let mut target_granted = false;
    for line in lines {
        if line.strip_prefix(&target).is_some() && !line.ends_with(" WITH GRANT OPTION") {
            target_granted = true;
        } else if !line.starts_with("GRANT USAGE ON *.* TO ")
            || line.ends_with(" WITH GRANT OPTION")
        {
            return false;
        }
    }
    target_granted
}

async fn stage_restore(file: File) -> Result<File> {
    // The mysql client reads one input stream. Prefix a session-scoped lock
    // and target check before any dump statements, and abort on guard errors.
    let guard = format!(
        "SET @fastdb_locked = GET_LOCK(CONCAT('fastdb_restore:', LEFT(SHA2(LOWER(DATABASE()), 256), 48)), 0);
         SET @fastdb_target = CASE WHEN NOT (@fastdb_locked <=> 1) THEN 'Another FastDB restore is running'
            WHEN {TARGET_POPULATED} THEN 'MySQL restore requires an empty database' ELSE 'empty' END;
         SELECT @fastdb_target;
         DO JSON_EXTRACT(IF(@fastdb_target = 'empty', '{{}}', 'FastDB restore blocked'), '$');
         "
    );
    let mut input = tokio::fs::File::from_std(file);
    let mut staged = tokio::fs::File::from_std(
        tempfile::tempfile().context("Cannot create temporary MySQL restore input")?,
    );
    staged.write_all(guard.as_bytes()).await?;
    portable_dump(&mut input, &mut staged).await?;
    staged.flush().await?;
    staged.rewind().await?;
    Ok(staged.into_std().await)
}

fn dump_args(command: &mut Command, database: &str, scope: &[(String, String)]) -> Result<()> {
    ensure!(
        !database.is_empty() && !database.contains('\0'),
        "Choose a MySQL database"
    );
    command.args([
        "--single-transaction",
        "--quick",
        "--no-tablespaces",
        "--set-gtid-purged=OFF",
        "--hex-blob",
        "--skip-add-drop-table",
        "--verbose",
    ]);
    if scope.is_empty() {
        command.args(["--routines", "--events"]);
    }
    command.arg("--").arg(database);
    for (schema, table) in scope {
        ensure!(
            schema == database && !table.is_empty() && !table.contains('\0'),
            "MySQL backup tables must belong to the selected database"
        );
        command.arg(table);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_definers_preserve_quoted_data_and_other_comments() -> Result<()> {
        let input = b"/*!50013 DEFINER=`source``owner`@`localhost` SQL SECURITY DEFINER */\n/*!50003 CREATE*/ /*!50017 DEFINER=`source`@`host`*/ /*!50003 TRIGGER t BEFORE INSERT ON x FOR EACH ROW SET @x=1 */;\nINSERT INTO x VALUES ('/*!50013 DEFINER=`keep`@`data` */', 'line\n/*!50013 DEFINER=`keep`@`data` */');\n-- /*!50013 DEFINER=`keep`@`comment` */\n";
        let expected = b"/*!50013 DEFINER=CURRENT_USER SQL SECURITY DEFINER */\n/*!50003 CREATE*/ /*!50017 DEFINER=CURRENT_USER*/ /*!50003 TRIGGER t BEFORE INSERT ON x FOR EACH ROW SET @x=1 */;\nINSERT INTO x VALUES ('/*!50013 DEFINER=`keep`@`data` */', 'line\n/*!50013 DEFINER=`keep`@`data` */');\n-- /*!50013 DEFINER=`keep`@`comment` */\n";
        db_wire::get_runtime().block_on(async {
            let mut output = Vec::new();
            portable_dump(&input[..], &mut output).await?;
            assert_eq!(output, expected);
            Ok(())
        })
    }

    #[test]
    fn portable_definers_reject_truncated_accounts() {
        db_wire::get_runtime().block_on(async {
            for dump in [
                &b"/*!50013 DEFINER=`unfinished"[..],
                &b"/*!50017 DEFINER=`user` `host`*/"[..],
                &b"/*!50017 DEFINER=`user`@`host"[..],
            ] {
                let mut output = Vec::new();
                assert!(portable_dump(dump, &mut output).await.is_err());
            }
        });
    }

    #[test]
    fn portable_definers_preserve_literal_backslashes_in_backticks() -> Result<()> {
        db_wire::get_runtime().block_on(async {
            let input = b"CREATE TABLE `table\\`(id INT);\n/*!50013 DEFINER=`owner\\`@`host\\` SQL SECURITY DEFINER */";
            let mut output = Vec::new();
            portable_dump(&input[..], &mut output).await?;
            assert_eq!(output, b"CREATE TABLE `table\\`(id INT);\n/*!50013 DEFINER=CURRENT_USER SQL SECURITY DEFINER */");
            Ok(())
        })
    }

    #[test]
    fn portable_definers_rewrite_create_routine_headers() -> Result<()> {
        db_wire::get_runtime().block_on(async {
            let input = b"CREATE DEFINER=`owner`@`localhost` PROCEDURE p() SELECT 'CREATE DEFINER=`data`@`data`';";
            let mut output = Vec::new();
            portable_dump(&input[..], &mut output).await?;
            assert_eq!(output, b"CREATE DEFINER=CURRENT_USER PROCEDURE p() SELECT 'CREATE DEFINER=`data`@`data`';");
            Ok(())
        })
    }

    #[test]
    fn restore_grants_require_literal_schema_scope_and_reject_broader_access() {
        let usage = "GRANT USAGE ON *.* TO `fixture`@`%`";
        let report =
            |partial, grant: &str| format!("{partial}\n{usage}\n{grant}\nfastdb_grants_end\n");
        let target = "GRANT ALL PRIVILEGES ON `restore_db`.* TO `fixture`@`%`";
        let escaped = "GRANT ALL PRIVILEGES ON `restore\\_db`.* TO `fixture`@`%`";
        assert!(restore_grants_allowed(&report(1, target), "restore_db"));
        assert!(restore_grants_allowed(&report(0, escaped), "restore_db"));
        assert!(!restore_grants_allowed(&report(0, target), "restore_db"));
        for broader in [
            "GRANT INSERT ON `source_db`.* TO `fixture`@`%`",
            "GRANT FILE ON *.* TO `fixture`@`%`",
            "GRANT `writer_role`@`%` TO `fixture`@`%`",
        ] {
            assert!(!restore_grants_allowed(
                &report(1, &format!("{target}\n{broader}")),
                "restore_db"
            ));
        }
        assert!(!restore_grants_allowed(
            &format!("{target}\nfastdb_grants_end\n"),
            "restore_db"
        ));
    }

    #[test]
    fn live_backup_restore_and_maintenance() -> Result<()> {
        let Some(directory) = std::env::var_os("FASTDB_TEST_P202_MYSQL_BIN") else {
            return Ok(());
        };
        let directory = Path::new(&directory);
        let tls_ca = std::env::var("FASTDB_TEST_P202_MYSQL_TLS_CA").ok();
        let config = ConnectionConfig {
            driver: DbDriverKind::MySql,
            host: if tls_ca.is_some() {
                "localhost"
            } else {
                "127.0.0.1"
            }
            .into(),
            port: 3306,
            database: "source_db".into(),
            user: "p202_fixture".into(),
            password: Some("fixture-secret".into()),
            use_unix_socket: false,
            tls_enabled: tls_ca.is_some(),
            tls_ca_cert: tls_ca,
            ..Default::default()
        };
        let outputs = tempfile::tempdir()?;
        let full = outputs.path().join("full.sql");
        let scoped = outputs.path().join("scoped.sql");
        let (_cancel, receiver) = watch::channel(false);
        let (logs, details) = watch::channel(String::new());
        db_wire::get_runtime().block_on(async {
            let connection = MysqlConnection::new(&config).await?;
            if config.tls_enabled {
                let mut check = connection.client(Some(directory), "source_db")?;
                check.args(["--execute", "SHOW SESSION STATUS LIKE 'Ssl_cipher'"]);
                let result = check.output()?;
                ensure!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
                let text = String::from_utf8(result.stdout)?;
                let cipher = text.trim().strip_prefix("Ssl_cipher\t").context("Missing TLS cipher")?;
                ensure!(!cipher.is_empty(), "Native MySQL connection did not negotiate TLS");
                let mismatched = MysqlConnection::new(&ConnectionConfig {
                    host: "127.0.0.1".into(),
                    ..config.clone()
                }).await?;
                let mut check = mismatched.client(Some(directory), "source_db")?;
                check.args(["--execute", "SELECT 1"]);
                let result = check.output()?;
                ensure!(!result.status.success(), "Native MySQL accepted a mismatched certificate hostname");
                assert!(String::from_utf8_lossy(&result.stderr).contains("SSL"));
            }
            let mut create_view = connection.client(Some(directory), "source_db")?;
            create_view.args([
                "--execute",
                "CREATE VIEW fixture_view AS SELECT id FROM fixture;
                 CREATE TRIGGER fixture_trigger BEFORE INSERT ON fixture FOR EACH ROW SET NEW.note=NEW.note;
                 CREATE PROCEDURE fixture_procedure() SELECT 42 AS answer;
                 CREATE EVENT fixture_event ON SCHEDULE EVERY 1 DAY DO SET @fastdb_fixture_event=1",
            ]);
            let created = create_view.output()?;
            ensure!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));
            for (archive, database, scope) in [
                (&full, "restore_db", vec![]),
                (
                    &scoped,
                    "scoped_db",
                    vec![("source_db".into(), "fixture".into())],
                ),
            ] {
                assert_eq!(
                    connection
                        .backup(
                            Some(directory),
                            archive,
                            &scope,
                            receiver.clone(),
                            logs.clone()
                        )
                        .await
                        .with_context(|| details.borrow().clone())?,
                    Outcome::Completed
                );
                assert_eq!(
                    MysqlConnection::new(&ConnectionConfig {
                        database: database.into(),
                        user: format!("p202_{database}"),
                        ..config.clone()
                    })
                    .await?
                    .restore(
                        Some(directory),
                        archive,
                        database,
                        receiver.clone(),
                        logs.clone()
                    )
                    .await
                    .with_context(|| details.borrow().clone())?,
                    Outcome::Completed
                );
                let target = MysqlConnection::new(&ConnectionConfig {
                    database: database.into(),
                    user: format!("p202_{database}"),
                    ..config.clone()
                })
                .await?;
                let mut query = target.client(Some(directory), database)?;
                query.args([
                    "--execute",
                    "SELECT id, note IS NULL, HEX(note), HEX(payload) FROM fixture ORDER BY id",
                ]);
                let result = query.output()?;
                ensure!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(result.stdout, b"1\t1\tNULL\t00FF\n2\t0\t\tNULL\n");
                let mut query = target.client(Some(directory), database)?;
                query.args(["--execute", "SHOW TABLES"]);
                let result = query.output()?;
                ensure!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(
                    String::from_utf8(result.stdout)?.lines().count(),
                    if scope.is_empty() { 3 } else { 1 }
                );
                if scope.is_empty() {
                    let mut query = target.client(Some(directory), database)?;
                    query.args(["--execute", "SELECT id FROM fixture_view ORDER BY id"]);
                    let result = query.output()?;
                    ensure!(
                        result.status.success(),
                        "{}",
                        String::from_utf8_lossy(&result.stderr)
                    );
                    assert_eq!(result.stdout, b"1\n2\n");
                    let mut query = target.client(Some(directory), database)?;
                    query.args(["--execute",
                        "CALL fixture_procedure();
                         SELECT COUNT(*) FROM information_schema.triggers WHERE trigger_schema=DATABASE();
                         SELECT COUNT(*) FROM information_schema.events WHERE event_schema=DATABASE()"]);
                    let result = query.output()?;
                    ensure!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
                    assert_eq!(result.stdout, b"42\n1\n1\n");
                }
                let rejected = target
                    .restore(
                        Some(directory),
                        archive,
                        database,
                        receiver.clone(),
                        logs.clone(),
                    )
                    .await;
                assert!(rejected.is_err());
                assert!(details.borrow().contains("empty database"));
            }
            let scoped_connection = |database: &str| ConnectionConfig {
                database: database.into(),
                user: format!("p202_{database}"),
                ..config.clone()
            };
            let outside = MysqlConnection::new(&scoped_connection("confined_db")).await?;
            let cross_schema = outputs.path().join("cross.sql");
            for sql in [
                "USE source_db; UPDATE fixture SET note = 'wrong' WHERE id = 1;",
                "UPDATE source_db.fixture SET note = 'wrong' WHERE id = 1;",
            ] {
                std::fs::write(&cross_schema, sql)?;
                assert!(
                    outside
                        .restore(
                            Some(directory),
                            &cross_schema,
                            "confined_db",
                            receiver.clone(),
                            logs.clone(),
                        )
                        .await
                        .is_err()
                );
                assert!(details.borrow().contains("denied"));
            }
            let mut check_source = connection.client(Some(directory), "source_db")?;
            check_source.args(["--execute", "SELECT note IS NULL FROM fixture WHERE id = 1"]);
            let result = check_source.output()?;
            ensure!(result.status.success());
            assert_eq!(result.stdout, b"1\n");
            let rejected = connection
                .restore(
                    Some(directory),
                    &cross_schema,
                    "restore_db",
                    receiver.clone(),
                    logs.clone(),
                )
                .await
                .unwrap_err();
            assert!(rejected.to_string().contains("privileges confined"));

            let race = MysqlConnection::new(&scoped_connection("race_db")).await?;
            let left = outputs.path().join("race-left.sql");
            let right = outputs.path().join("race-right.sql");
            std::fs::write(&left, "CREATE TABLE a (id INT PRIMARY KEY);")?;
            std::fs::write(&right, "CREATE TABLE b (id INT PRIMARY KEY);")?;
            let (left_logs, _left_detail) = watch::channel(String::new());
            let (right_logs, _right_detail) = watch::channel(String::new());
            let (left, right) = tokio::join!(
                race.restore(
                    Some(directory),
                    &left,
                    "race_db",
                    receiver.clone(),
                    left_logs
                ),
                race.restore(
                    Some(directory),
                    &right,
                    "race_db",
                    receiver.clone(),
                    right_logs
                ),
            );
            assert_eq!(
                [left, right]
                    .into_iter()
                    .filter(|result| matches!(result, Ok(Outcome::Completed)))
                    .count(),
                1
            );
            for operation in [Maintenance::Analyze, Maintenance::Vacuum] {
                assert_eq!(
                    connection
                        .maintain(
                            Some(directory),
                            operation,
                            "fixture",
                            receiver.clone(),
                            logs.clone()
                        )
                        .await
                        .with_context(|| details.borrow().clone())?,
                    Outcome::Completed
                );
            }
            Ok(())
        })
    }

    #[test]
    fn literal_table_arguments_follow_the_option_separator() -> Result<()> {
        let mut command = Command::new("/fixtures/mysqldump");
        dump_args(
            &mut command,
            "db",
            &[("db".into(), "--lock-all-tables".into())],
        )?;
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(&args[args.len() - 3..], ["--", "db", "--lock-all-tables"]);
        assert!(!args.contains(&std::ffi::OsStr::new("--routines")));
        assert!(dump_args(&mut command, "db", &[("other".into(), "t".into())]).is_err());
        Ok(())
    }

    #[test]
    fn private_option_file_keeps_password_out_of_command_arguments() -> Result<()> {
        let config = ConnectionConfig {
            driver: DbDriverKind::MySql,
            database: "fixture".into(),
            password: Some("quoted \"\\\n# secret".into()),
            use_unix_socket: false,
            ..Default::default()
        };
        let connection = db_wire::get_runtime().block_on(MysqlConnection::new(&config))?;
        let command = connection.command(Path::new("/fixtures/mysql"));
        let args: Vec<_> = command.get_args().collect();
        assert!(args[0].to_string_lossy().starts_with("--defaults-file="));
        assert!(
            !args
                .iter()
                .any(|arg| arg.to_string_lossy().contains("secret"))
        );
        assert_eq!(
            std::fs::read_to_string(connection.options.path())?,
            "[client]\npassword=\"quoted \\\"\\\\\\n# secret\"\n"
        );
        Ok(())
    }
}
