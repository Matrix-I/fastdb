use super::process::{self, BackupOutput, Outcome};
#[cfg(test)]
use super::tools::Tool;
use super::{BackupArtifact, Maintenance};
use anyhow::{Context, Result, ensure};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::sync::watch;

const JOB_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

pub(crate) async fn backup(
    tool: &Path,
    database: &Path,
    destination: &Path,
    cancel: watch::Receiver<bool>,
    logs: watch::Sender<String>,
) -> Result<Outcome> {
    let database = database_file(database)?;
    let artifact = BackupArtifact::stage(destination)?;
    let command = copy_command(tool, &database, &artifact)?;
    logs.send_replace("Backing up the SQLite database…".to_owned());
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

pub(crate) async fn backup_scoped(
    tool: &Path,
    database: &Path,
    destination: &Path,
    scope: &[(String, String)],
    cancel: watch::Receiver<bool>,
    logs: watch::Sender<String>,
) -> Result<Outcome> {
    let plan = scoped_prune_plan(scope)?;
    let artifact = BackupArtifact::stage(destination)?;
    let scratch = tempfile::tempdir().context("Cannot create SQLite backup staging directory")?;
    let snapshot = scratch.path().join("snapshot.sqlite");
    if backup(tool, database, &snapshot, cancel.clone(), logs.clone()).await? == Outcome::Canceled {
        return Ok(Outcome::Canceled);
    }

    let script = scratch.path().join("prune.sql");
    let staged_script = BackupArtifact::stage(&script)?;
    let mut generate = command_for(tool);
    generate.arg("-readonly").arg(&snapshot).arg(plan);
    logs.send_replace("Preparing selected SQLite objects…".to_owned());
    if process::run(
        generate,
        None,
        Some(BackupOutput::Stdout(staged_script)),
        cancel.clone(),
        logs.clone(),
        JOB_TIMEOUT,
    )
    .await?
        == Outcome::Canceled
    {
        return Ok(Outcome::Canceled);
    }

    // Rewrite the private snapshot so deleted objects cannot remain in free
    // pages of the published archive.
    logs.send_replace("Compacting the scoped SQLite backup…".to_owned());
    if process::run(
        command(tool, &snapshot),
        Some(File::open(script)?),
        None,
        cancel.clone(),
        logs.clone(),
        JOB_TIMEOUT,
    )
    .await?
        == Outcome::Canceled
    {
        return Ok(Outcome::Canceled);
    }

    let copy = copy_command(tool, &snapshot, &artifact)?;
    process::run(
        copy,
        None,
        Some(BackupOutput::NativeFile(artifact)),
        cancel,
        logs,
        JOB_TIMEOUT,
    )
    .await
}

fn scoped_prune_plan(scope: &[(String, String)]) -> Result<String> {
    ensure!(!scope.is_empty(), "Choose a SQLite table or view");
    let mut values = Vec::with_capacity(scope.len());
    for (schema, name) in scope {
        ensure!(
            schema == "main" && !name.is_empty() && !name.contains('\0'),
            "Scoped SQLite backup requires objects in the main database"
        );
        let hex = db_wire::value::hex_text(name.as_bytes());
        values.push(format!("(CAST(X'{}' AS TEXT))", &hex[2..]));
    }
    Ok(format!(
        r#"
        WITH selected(name) AS (VALUES {}),
        relations AS MATERIALIZED (
            SELECT name, type FROM pragma_table_list WHERE schema = 'main'
        ),
        steps(ord, sql) AS (
            SELECT 0, CASE WHEN EXISTS (
                SELECT selected.name FROM selected
                LEFT JOIN relations ON relations.name = selected.name
                    AND relations.type IN ('table', 'view')
                    AND relations.name NOT GLOB 'sqlite_*'
                WHERE relations.name IS NULL
            ) THEN 'CREATE TEMP TABLE fastdb_scope_guard (
                        valid INTEGER CONSTRAINT backup_requires_existing_table_or_view CHECK (valid = 1)
                    ); INSERT INTO fastdb_scope_guard VALUES (0);'
              ELSE 'PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;' END
            UNION ALL
            SELECT CASE s.type WHEN 'trigger' THEN 1 WHEN 'view' THEN 2 ELSE 3 END,
                   'DROP ' || upper(s.type) || ' IF EXISTS main."' ||
                   replace(s.name, '"', '""') || '";'
            FROM sqlite_schema AS s
            WHERE s.type IN ('trigger', 'view', 'table')
              AND s.name NOT GLOB 'sqlite_*'
              AND s.tbl_name NOT IN (SELECT name FROM selected)
              AND s.name NOT IN (SELECT name FROM relations WHERE type = 'shadow')
            UNION ALL
            SELECT 4, 'DELETE FROM main.sqlite_sequence WHERE name IS NULL OR name NOT IN (' ||
                      (SELECT group_concat(quote(name), ',') FROM selected) || ');'
            WHERE EXISTS (SELECT 1 FROM relations WHERE name = 'sqlite_sequence')
            UNION ALL
            SELECT 4, 'DELETE FROM main."' || name || '";'
            FROM relations WHERE name IN ('sqlite_stat1', 'sqlite_stat4')
            UNION ALL
            SELECT 5, 'COMMIT; VACUUM;'
        )
        SELECT sql FROM steps ORDER BY ord;"#,
        values.join(", "),
    ))
}

pub(crate) async fn restore(
    tool: &Path,
    archive: &Path,
    destination: &Path,
    cancel: watch::Receiver<bool>,
    logs: watch::Sender<String>,
) -> Result<Outcome> {
    match destination.symlink_metadata() {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return restore_new(tool, archive, destination, cancel, logs).await;
        }
        Err(error) => return Err(error).context("Cannot check the SQLite restore destination"),
        Ok(metadata) => ensure!(metadata.is_file(), "Choose a regular SQLite database file"),
    }
    let destination = database_file(destination)?;
    ensure!(
        archive.canonicalize()? != destination,
        "Choose a separate SQLite restore database"
    );
    let staging =
        tempfile::tempdir().context("Cannot create a SQLite restore staging directory")?;
    let image = staging.path().join("validated.sqlite");
    if restore_new(tool, archive, &image, cancel.clone(), logs.clone()).await? == Outcome::Canceled
    {
        return Ok(Outcome::Canceled);
    }
    let mut command = command_for(tool);
    command.current_dir(staging.path());
    // Exclusive locking retains the write lock after COMMIT on this connection.
    // The shell's backup API needs the guard transaction closed before .restore.
    command
        .args(["-ifexists", "-nofollow"])
        .arg(destination)
        .args([
            "PRAGMA locking_mode=EXCLUSIVE",
            "BEGIN EXCLUSIVE;
             CREATE TEMP TABLE fastdb_restore_guard(
                 empty_target INTEGER CONSTRAINT restore_requires_empty_database CHECK (empty_target = 1)
             );
             INSERT INTO fastdb_restore_guard
             SELECT NOT EXISTS (SELECT 1 FROM main.sqlite_schema);
             COMMIT;",
            ".restore validated.sqlite",
        ]);
    logs.send_replace(
        "Checking and restoring the existing SQLite database under an exclusive lock…".to_owned(),
    );
    process::run(command, None, None, cancel, logs, JOB_TIMEOUT).await
}

async fn restore_new(
    tool: &Path,
    archive: &Path,
    destination: &Path,
    cancel: watch::Receiver<bool>,
    logs: watch::Sender<String>,
) -> Result<Outcome> {
    let archive = database_file(archive)?;
    let mut header = [0; SQLITE_HEADER.len()];
    File::open(&archive)?
        .read_exact(&mut header)
        .context("Cannot read the SQLite archive header")?;
    ensure!(
        &header == SQLITE_HEADER,
        "The archive is not a native SQLite database"
    );

    let artifact = BackupArtifact::stage(destination)?;
    let command = copy_command(tool, &archive, &artifact)?;
    logs.send_replace("Restoring into a staged SQLite database…".to_owned());
    if process::run(
        command,
        None,
        None,
        cancel.clone(),
        logs.clone(),
        JOB_TIMEOUT,
    )
    .await?
        == Outcome::Canceled
    {
        return Ok(Outcome::Canceled);
    }

    logs.send_replace("Checking the restored SQLite database…".to_owned());
    let (report, result) = watch::channel(String::new());
    let mut check = command_for(tool);
    check
        .arg("-readonly")
        .arg(artifact.path())
        .arg("PRAGMA quick_check");
    if process::run(check, None, None, cancel.clone(), report, JOB_TIMEOUT).await?
        == Outcome::Canceled
    {
        return Ok(Outcome::Canceled);
    }
    ensure!(
        result.borrow().trim() == "ok",
        "The restored SQLite database failed its integrity check"
    );
    if *cancel.borrow() || cancel.has_changed().is_err() {
        return Ok(Outcome::Canceled);
    }
    tokio::task::spawn_blocking(move || artifact.publish()).await??;
    logs.send_replace("SQLite integrity check passed.".to_owned());
    Ok(Outcome::Completed)
}

pub(crate) async fn maintain(
    tool: &Path,
    database: &Path,
    operation: Maintenance,
    cancel: watch::Receiver<bool>,
    logs: watch::Sender<String>,
) -> Result<Outcome> {
    let database = database_file(database)?;
    let sql = match operation {
        Maintenance::Vacuum => "VACUUM",
        Maintenance::Analyze => "ANALYZE",
        Maintenance::Reindex => "REINDEX",
    };
    let mut command = command(tool, &database);
    command.arg(sql);
    logs.send_replace(format!("Running SQLite {sql}…"));
    process::run(command, None, None, cancel, logs, JOB_TIMEOUT).await
}

fn copy_command(tool: &Path, source: &Path, artifact: &BackupArtifact) -> Result<Command> {
    let stage = artifact.path();
    let name = stage
        .file_name()
        .and_then(|name| name.to_str())
        .context("Invalid staged file name")?;
    // The shell parses dot-command arguments itself. Use only our generated
    // basename there, and pass user-controlled paths as process arguments.
    ensure!(
        name.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
    );
    let mut command = command_for(tool);
    command.current_dir(stage.parent().context("Missing staged file directory")?);
    command
        .arg("-readonly")
        .arg(source)
        .arg(format!(".backup {name}"));
    Ok(command)
}

fn database_file(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .context("Cannot find the SQLite database file")?;
    ensure!(path.is_file(), "SQLite requires a regular database file");
    Ok(path)
}

fn command(tool: &Path, database: &Path) -> Command {
    let mut command = command_for(tool);
    command.arg(database);
    command
}

fn command_for(tool: &Path) -> Command {
    let mut command = Command::new(tool);
    command.args([
        "-batch",
        "-bail",
        "-init",
        "/dev/null",
        "-cmd",
        ".timeout 5000",
    ]);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(tool: &Path, database: &Path, sql: &str) -> Result<String> {
        let output = command(tool, database).arg(sql).output()?;
        ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }

    #[test]
    fn scoped_native_backup_keeps_exact_rows_indexes_and_sequences() -> Result<()> {
        let Some(bin_directory) = std::env::var_os("FASTDB_TEST_SQLITE_CLI_DIR") else {
            return Ok(());
        };
        let bin_directory = PathBuf::from(bin_directory);
        let tool = Tool::Sqlite.resolve(Some(&bin_directory))?;
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source.sqlite");
        let archive = directory.path().join("scoped.sqlite");
        let restored = directory.path().join("restored.sqlite");
        query(
            &tool,
            &source,
            r#"CREATE TABLE "fixture_'""x" (id INTEGER PRIMARY KEY AUTOINCREMENT, note TEXT, bytes BLOB);
               CREATE TABLE "fixturey'""x" (private TEXT);
               CREATE INDEX keep_index ON "fixture_'""x"(note);
               CREATE INDEX remove_index ON "fixturey'""x"(private);
               CREATE VIEW remove_view AS SELECT private FROM "fixturey'""x";
               INSERT INTO "fixture_'""x" VALUES (1,char(0,10),x'00ff'),(20,NULL,NULL);
               DELETE FROM "fixture_'""x" WHERE id = 20;
               INSERT INTO "fixturey'""x" VALUES ('excluded-private-fixture-value');
               INSERT INTO sqlite_sequence VALUES ('excluded-private-fixture-value', 99);"#,
        )?;
        let config = db_wire::ConnectionConfig {
            database: source.to_string_lossy().into_owned(),
            driver: db_wire::DbDriverKind::Sqlite,
            ..Default::default()
        };
        let (_cancel, receiver) = watch::channel(false);
        let (logs, details) = watch::channel(String::new());
        db_wire::get_runtime().block_on(async {
            use crate::native_backup::{Operation, RestoreTarget, execute};
            assert_eq!(
                execute(
                    config.clone(),
                    Some(bin_directory.clone()),
                    Operation::Backup {
                        destination: archive.clone(),
                        scope: vec![("main".into(), "fixture_'\"x".into())],
                    },
                    receiver.clone(),
                    logs.clone(),
                )
                .await
                .with_context(|| details.borrow().clone())?,
                Outcome::Completed
            );
            assert_eq!(
                execute(
                    config,
                    Some(bin_directory),
                    Operation::Restore {
                        archive: archive.clone(),
                        target: RestoreTarget::SqliteFile(restored.clone()),
                    },
                    receiver,
                    logs,
                )
                .await
                .with_context(|| details.borrow().clone())?,
                Outcome::Completed
            );
            Ok::<_, anyhow::Error>(())
        })?;
        assert_eq!(
            query(
                &tool,
                &restored,
                r#"SELECT id, hex(note), hex(bytes) FROM "fixture_'""x""#
            )?,
            "1|000A|00FF\n"
        );
        assert_eq!(
            query(&tool, &restored, "SELECT seq FROM sqlite_sequence")?,
            "20\n"
        );
        assert_eq!(
            query(
                &tool,
                &restored,
                "SELECT name FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name"
            )?,
            "fixture_'\"x\nkeep_index\n"
        );
        let excluded = b"excluded-private-fixture-value";
        assert!(
            !std::fs::read(&archive)?
                .windows(excluded.len())
                .any(|bytes| bytes == excluded)
        );
        assert_eq!(
            query(&tool, &source, r#"SELECT private FROM "fixturey'""x""#)?,
            "excluded-private-fixture-value\n"
        );
        Ok(())
    }

    #[test]
    fn native_round_trip_maintenance_and_restore_rejections() -> anyhow::Result<()> {
        let Some(directory) = std::env::var_os("FASTDB_TEST_SQLITE_CLI_DIR") else {
            eprintln!("Set FASTDB_TEST_SQLITE_CLI_DIR to run the native SQLite round trip");
            return Ok(());
        };
        let tool = Tool::Sqlite.resolve(Some(Path::new(&directory)))?;
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source ' ; \" .db");
        let archive = directory.path().join("backup ' ; \" .sqlite");
        let restored = directory.path().join("restored ' ; \" .db");
        let empty = directory.path().join("empty ' ; \" .db");
        query(&tool, &empty, "PRAGMA user_version=1")?;
        query(
            &tool,
            &source,
            "CREATE TABLE fixture(id INTEGER PRIMARY KEY, note TEXT, bytes BLOB);
             CREATE INDEX fixture_note ON fixture(note);
             INSERT INTO fixture VALUES (1,NULL,x'00ff'),(2,char(0,10),NULL);",
        )?;
        let (_cancel, receiver) = watch::channel(false);
        let (logs, _held) = watch::channel(String::new());

        db_wire::get_runtime().block_on(async {
            assert_eq!(
                backup(&tool, &source, &archive, receiver.clone(), logs.clone()).await?,
                Outcome::Completed
            );
            assert_eq!(
                restore_new(&tool, &archive, &restored, receiver.clone(), logs.clone()).await?,
                Outcome::Completed
            );
            assert_eq!(
                restore(&tool, &archive, &empty, receiver.clone(), logs.clone()).await?,
                Outcome::Completed
            );
            assert!(
                restore(&tool, &archive, &source, receiver.clone(), logs.clone())
                    .await
                    .is_err()
            );
            for operation in [
                Maintenance::Vacuum,
                Maintenance::Analyze,
                Maintenance::Reindex,
            ] {
                assert_eq!(
                    maintain(&tool, &restored, operation, receiver.clone(), logs.clone()).await?,
                    Outcome::Completed
                );
            }
            assert!(
                restore_new(&tool, &archive, &source, receiver.clone(), logs.clone())
                    .await
                    .is_err()
            );
            let corrupt = directory.path().join("corrupt");
            let missing = directory.path().join("missing");
            std::fs::write(&corrupt, b"not a SQLite database")?;
            assert!(
                restore_new(&tool, &corrupt, &missing, receiver, logs)
                    .await
                    .is_err()
            );
            assert!(!missing.exists());
            Ok::<_, anyhow::Error>(())
        })?;

        let read = "SELECT id, typeof(note), hex(note), typeof(bytes), hex(bytes)
                    FROM fixture ORDER BY id";
        let expected = "1|null||blob|00FF\n2|text|000A|null|\n";
        assert_eq!(query(&tool, &source, read)?, expected);
        assert_eq!(query(&tool, &restored, read)?, expected);
        assert_eq!(query(&tool, &empty, read)?, expected);
        assert_eq!(
            query(
                &tool,
                &restored,
                "SELECT idx FROM sqlite_stat1 WHERE tbl = 'fixture'"
            )?,
            "fixture_note\n"
        );
        Ok(())
    }
}
