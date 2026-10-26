//! SQL scripts saved as `.sql` files, one folder per connection:
//! `<config_dir>/scripts/<connection id>/Script-<n>.sql`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

/// A saved script of one connection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFile {
    pub path: PathBuf,
    pub connection_id: String,
    /// File stem, shown as the tab title ("Script-3")
    pub name: String,
    pub modified: SystemTime,
    /// First non-empty line, trimmed, for the chooser's Info column
    pub preview: String,
}

/// Reads and writes the script files under one root folder
pub struct ScriptStore {
    root: PathBuf,
}

const PREFIX: &str = "Script-";

impl ScriptStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// `<config_dir>/scripts`
    pub fn default_root() -> PathBuf {
        platform_hal::paths::config_dir().join("scripts")
    }

    fn dir(&self, connection_id: &str) -> PathBuf {
        self.root.join(connection_id)
    }

    /// Number of a `Script-<n>.sql` file name
    fn number(file_name: &str) -> Option<u64> {
        file_name.strip_prefix(PREFIX)?.strip_suffix(".sql")?.parse().ok()
    }

    fn list_dir(&self, connection_id: &str, out: &mut Vec<ScriptFile>) {
        let Ok(entries) = std::fs::read_dir(self.dir(connection_id)) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if Self::number(file_name).is_none() {
                continue;
            }
            let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            let preview = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string))
                .unwrap_or_default();
            out.push(ScriptFile {
                name: file_name.trim_end_matches(".sql").to_string(),
                connection_id: connection_id.to_string(),
                path,
                modified,
                preview,
            });
        }
    }

    /// Scripts of one connection, or of all connections for `None`, newest first
    pub fn list(&self, connection_id: Option<&str>) -> Vec<ScriptFile> {
        let mut out = Vec::new();
        match connection_id {
            Some(id) => self.list_dir(id, &mut out),
            None => {
                for entry in std::fs::read_dir(&self.root).into_iter().flatten().flatten() {
                    if entry.path().is_dir() {
                        if let Some(id) = entry.file_name().to_str() {
                            self.list_dir(id, &mut out);
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.name.cmp(&a.name)));
        out
    }

    pub fn most_recent(&self, connection_id: &str) -> Option<ScriptFile> {
        self.list(Some(connection_id)).into_iter().next()
    }

    /// Creates an empty `Script-<n>.sql` numbered after the connection's highest script
    pub fn create(&self, connection_id: &str) -> Result<ScriptFile> {
        let dir = self.dir(connection_id);
        std::fs::create_dir_all(&dir).with_context(|| format!("Cannot create {}", dir.display()))?;
        let next = std::fs::read_dir(&dir)?
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(Self::number))
            .max()
            .unwrap_or(0)
            + 1;
        let name = format!("{PREFIX}{next}");
        let path = dir.join(format!("{name}.sql"));
        std::fs::write(&path, "").with_context(|| format!("Cannot create {}", path.display()))?;
        Ok(ScriptFile {
            path,
            connection_id: connection_id.to_string(),
            name,
            modified: SystemTime::now(),
            preview: String::new(),
        })
    }

    pub fn read(&self, script: &ScriptFile) -> Result<String> {
        std::fs::read_to_string(&script.path).with_context(|| format!("Cannot read {}", script.path.display()))
    }

    pub fn write(&self, path: &Path, text: &str) -> Result<()> {
        std::fs::write(path, text).with_context(|| format!("Cannot save {}", path.display()))
    }
}

/// Scripts whose name or preview contains `query` (case-insensitive); all for an empty query
pub fn filter_scripts<'a>(scripts: &'a [ScriptFile], query: &str) -> Vec<&'a ScriptFile> {
    let query = query.trim().to_lowercase();
    scripts
        .iter()
        .filter(|s| query.is_empty() || s.name.to_lowercase().contains(&query) || s.preview.to_lowercase().contains(&query))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{filter_scripts, ScriptStore};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fastdb-scripts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn touch(path: &std::path::Path, secs_ago: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(secs_ago)).unwrap();
    }

    #[test]
    fn new_scripts_continue_after_the_highest_number() {
        let root = temp_root("numbering");
        let store = ScriptStore::new(root.clone());
        assert_eq!(store.create("pg").unwrap().name, "Script-1");
        assert_eq!(store.create("pg").unwrap().name, "Script-2");
        std::fs::write(root.join("pg").join("Script-38.sql"), "").unwrap();
        assert_eq!(store.create("pg").unwrap().name, "Script-39");
        // Numbering is per connection
        assert_eq!(store.create("mysql").unwrap().name, "Script-1");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_is_newest_first_with_a_preview_and_per_connection() {
        let root = temp_root("list");
        let store = ScriptStore::new(root.clone());
        let old = store.create("pg").unwrap();
        store.write(&old.path, "\n  SELECT 1;\nSELECT 2;").unwrap();
        touch(&old.path, 60);
        let new = store.create("pg").unwrap();
        store.write(&new.path, "drop index x;").unwrap();
        store.create("mysql").unwrap();

        let pg = store.list(Some("pg"));
        assert_eq!(pg.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Script-2", "Script-1"]);
        assert_eq!(pg[1].preview, "SELECT 1;");
        assert_eq!(pg[0].connection_id, "pg");
        assert_eq!(store.most_recent("pg").unwrap().name, "Script-2");
        assert_eq!(store.list(None).len(), 3);
        assert_eq!(store.read(&pg[1]).unwrap(), "\n  SELECT 1;\nSELECT 2;");
        assert!(store.most_recent("sqlite").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_script_files_are_ignored() {
        let root = temp_root("ignore");
        let store = ScriptStore::new(root.clone());
        store.create("pg").unwrap();
        std::fs::write(root.join("pg").join("notes.txt"), "x").unwrap();
        assert_eq!(store.list(Some("pg")).len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn filter_matches_name_or_preview_ignoring_case() {
        let root = temp_root("filter");
        let store = ScriptStore::new(root.clone());
        let a = store.create("pg").unwrap();
        store.write(&a.path, "explain analyze select 1").unwrap();
        touch(&a.path, 60);
        store.create("pg").unwrap();
        let all = store.list(Some("pg"));
        assert_eq!(filter_scripts(&all, "").len(), 2);
        assert_eq!(filter_scripts(&all, "EXPLAIN")[0].name, "Script-1");
        assert_eq!(filter_scripts(&all, "script-2")[0].name, "Script-2");
        assert!(filter_scripts(&all, "nothing").is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
