use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub(crate) enum Tool {
    PgDump,
    PgRestore,
    Psql,
    VacuumDb,
    ReindexDb,
    MysqlDump,
    Mysql,
    Sqlite,
}

impl Tool {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::PgDump => "pg_dump",
            Self::PgRestore => "pg_restore",
            Self::Psql => "psql",
            Self::VacuumDb => "vacuumdb",
            Self::ReindexDb => "reindexdb",
            Self::MysqlDump => "mysqldump",
            Self::Mysql => "mysql",
            Self::Sqlite => "sqlite3",
        }
    }

    pub(crate) fn resolve(self, directory: Option<&Path>) -> Result<PathBuf> {
        if let Some(directory) = directory {
            return resolve_in(self, [directory]);
        }

        // Desktop launches may inherit a smaller PATH than interactive shells.
        let search_path = std::env::var_os("PATH").unwrap_or_default();
        let search_dirs: Vec<_> = std::env::split_paths(&search_path).collect();
        let system_dirs = [
            "/opt/homebrew/bin",
            "/opt/homebrew/opt/libpq/bin",
            "/opt/homebrew/opt/mysql-client/bin",
            "/usr/local/bin",
            "/usr/local/opt/libpq/bin",
            "/usr/local/opt/mysql-client/bin",
            "/usr/bin",
        ];
        resolve_in(
            self,
            search_dirs
                .iter()
                .map(PathBuf::as_path)
                .chain(system_dirs.map(Path::new)),
        )
    }
}

fn resolve_in<'a>(tool: Tool, directories: impl IntoIterator<Item = &'a Path>) -> Result<PathBuf> {
    directories
        .into_iter()
        .filter(|directory| directory.is_absolute())
        .filter_map(|directory| directory.canonicalize().ok())
        .map(|directory| directory.join(format!("{}{}", tool.name(), std::env::consts::EXE_SUFFIX)))
        .find(|path| is_executable(path))
        .with_context(|| {
            format!(
                "Cannot find {}. Choose its executable directory.",
                tool.name()
            )
        })
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    metadata.is_file()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn executable(path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, "fixture")?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    #[test]
    fn checks_the_explicit_directory_without_falling_back() -> anyhow::Result<()> {
        let selected = tempfile::tempdir()?;
        let other = tempfile::tempdir()?;
        let fallback = other.path().join("pg_dump");
        executable(&fallback)?;

        assert!(resolve_in(Tool::PgDump, [selected.path()]).is_err());
        let chosen = selected.path().join("pg_dump");
        executable(&chosen)?;
        assert_eq!(
            resolve_in(Tool::PgDump, [selected.path(), other.path()])?,
            selected.path().canonicalize()?.join("pg_dump")
        );
        Ok(())
    }

    #[test]
    fn skips_nonexecutables_directories_and_relative_entries() -> anyhow::Result<()> {
        let invalid = tempfile::tempdir()?;
        let valid = tempfile::tempdir()?;
        let expected = valid.path().canonicalize()?.join("sqlite3");
        executable(&expected)?;
        let candidate = invalid.path().join("sqlite3");
        std::fs::create_dir(&candidate)?;
        assert_eq!(
            resolve_in(Tool::Sqlite, [Path::new("."), invalid.path(), valid.path()])?,
            expected
        );
        std::fs::remove_dir(&candidate)?;
        std::fs::write(candidate, "not executable")?;
        assert_eq!(
            resolve_in(Tool::Sqlite, [invalid.path(), valid.path()])?,
            expected
        );
        Ok(())
    }
}
