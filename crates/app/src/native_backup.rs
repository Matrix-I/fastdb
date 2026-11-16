use anyhow::{Context, Result, ensure};
use db_wire::{ConnectionConfig, DbDriverKind};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::sync::watch;

pub(crate) mod mysql;
pub(crate) mod postgres;
pub(crate) mod process;
pub(crate) mod sqlite;
pub(crate) mod tools;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Maintenance {
    Vacuum,
    Analyze,
    Reindex,
}

pub(crate) enum Operation {
    Backup {
        destination: PathBuf,
        scope: Vec<(String, String)>,
    },
    Restore {
        archive: PathBuf,
        target: RestoreTarget,
    },
    Maintain {
        operation: Maintenance,
        table: Option<String>,
    },
}

pub(crate) enum RestoreTarget {
    Database(String),
    PostgresSchema { database: String, schema: String },
    SqliteFile(PathBuf),
}

pub(crate) async fn execute(
    config: ConnectionConfig,
    tools: Option<PathBuf>,
    operation: Operation,
    cancel: watch::Receiver<bool>,
    logs: watch::Sender<String>,
) -> Result<process::Outcome> {
    ensure!(
        !config.read_only || matches!(operation, Operation::Backup { .. }),
        "This operation requires a writable profile"
    );
    let tools = tools.as_deref();
    match config.driver {
        DbDriverKind::Postgres => {
            let connection = postgres::PgConnection::new(&config).await?;
            match operation {
                Operation::Backup { destination, scope } => {
                    connection
                        .backup(tools, &destination, &scope, cancel, logs)
                        .await
                }
                Operation::Restore {
                    archive,
                    target: RestoreTarget::Database(database),
                } => {
                    connection
                        .restore(tools, &archive, &database, cancel, logs)
                        .await
                }
                Operation::Restore {
                    archive,
                    target: RestoreTarget::PostgresSchema { database, schema },
                } => {
                    connection
                        .restore_with_schema(
                            tools,
                            &archive,
                            &database,
                            Some(&schema),
                            cancel,
                            logs,
                        )
                        .await
                }
                Operation::Maintain { operation, .. } => {
                    connection.maintain(tools, operation, cancel, logs).await
                }
                _ => anyhow::bail!("Choose a PostgreSQL database as the restore target"),
            }
        }
        DbDriverKind::MySql => {
            let connection = mysql::MysqlConnection::new(&config).await?;
            match operation {
                Operation::Backup { destination, scope } => {
                    connection
                        .backup(tools, &destination, &scope, cancel, logs)
                        .await
                }
                Operation::Restore {
                    archive,
                    target: RestoreTarget::Database(database),
                } => {
                    connection
                        .restore(tools, &archive, &database, cancel, logs)
                        .await
                }
                Operation::Maintain { operation, table } => {
                    let table = table.as_deref().context("Select a MySQL table")?;
                    connection
                        .maintain(tools, operation, table, cancel, logs)
                        .await
                }
                _ => anyhow::bail!("Choose a MySQL database as the restore target"),
            }
        }
        DbDriverKind::Sqlite => {
            let database = Path::new(&config.database);
            let tool = tools::Tool::Sqlite.resolve(tools)?;
            match operation {
                Operation::Backup { destination, scope } => {
                    ensure!(
                        !config.database.is_empty() && config.database != ":memory:",
                        "SQLite native backup requires a database file"
                    );
                    if scope.is_empty() {
                        sqlite::backup(&tool, database, &destination, cancel, logs).await
                    } else {
                        sqlite::backup_scoped(&tool, database, &destination, &scope, cancel, logs)
                            .await
                    }
                }
                Operation::Restore {
                    archive,
                    target: RestoreTarget::SqliteFile(destination),
                } => sqlite::restore(&tool, &archive, &destination, cancel, logs).await,
                Operation::Maintain { operation, .. } => {
                    ensure!(
                        !config.database.is_empty() && config.database != ":memory:",
                        "SQLite native maintenance requires a database file"
                    );
                    sqlite::maintain(&tool, database, operation, cancel, logs).await
                }
                _ => anyhow::bail!("Choose a file as the SQLite restore target"),
            }
        }
        _ => anyhow::bail!("Native database tools are unavailable for this provider"),
    }
}

pub(crate) struct BackupArtifact {
    temporary: NamedTempFile,
    destination: PathBuf,
}

impl BackupArtifact {
    pub(crate) fn stage(destination: &Path) -> Result<Self> {
        let name = destination
            .file_name()
            .context("Backup needs a file name")?;
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent
            .canonicalize()
            .context("Cannot resolve the backup directory")?;
        let destination = parent.join(name);
        match fs::symlink_metadata(&destination) {
            Ok(_) => anyhow::bail!("Backup destination already exists"),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Cannot check the backup destination"),
        }
        let temporary = tempfile::Builder::new()
            .prefix(".fastdb-backup-")
            .tempfile_in(parent)
            .context("Cannot create a temporary backup file")?;
        Ok(Self {
            temporary,
            destination,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        self.temporary.path()
    }

    pub(crate) fn publish(self) -> Result<()> {
        let file = self.temporary.as_file();
        ensure!(file.metadata()?.len() > 0, "Native backup output is empty");
        file.sync_all().context("Cannot sync the backup file")?;
        self.temporary
            .persist_noclobber(&self.destination)
            .map_err(|error| error.error)
            .context("Cannot publish the backup without replacing an existing file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn publishes_native_bytes_and_removes_the_stage() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("backup");
        let artifact = BackupArtifact::stage(&target)?;
        let stage = artifact.path().to_owned();
        assert_eq!(
            stage.parent(),
            Some(directory.path().canonicalize()?.as_path())
        );
        fs::write(&stage, b"\0native backup\xff")?;

        artifact.publish()?;

        assert_eq!(fs::read(target)?, b"\0native backup\xff");
        assert!(!stage.exists());
        Ok(())
    }

    #[test]
    fn abandoned_or_empty_output_leaves_no_backup() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("backup");
        let artifact = BackupArtifact::stage(&target)?;
        let stage = artifact.path().to_owned();
        fs::write(&stage, b"partial output")?;
        drop(artifact);
        assert!(!stage.exists());

        let artifact = BackupArtifact::stage(&target)?;
        let stage = artifact.path().to_owned();
        assert!(artifact.publish().is_err());
        assert!(!stage.exists());
        assert!(!target.exists());
        Ok(())
    }

    #[test]
    fn never_replaces_an_existing_destination() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("backup");
        let artifact = BackupArtifact::stage(&target)?;
        let stage = artifact.path().to_owned();
        fs::write(&stage, b"new backup")?;
        fs::write(&target, b"existing backup")?;

        assert!(artifact.publish().is_err());

        assert_eq!(fs::read(target)?, b"existing backup");
        assert!(!stage.exists());
        Ok(())
    }
}
