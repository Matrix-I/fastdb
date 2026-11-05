use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_LAYOUT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(serde::Deserialize, serde::Serialize)]
pub(super) struct Position {
    pub schema: String,
    pub table: String,
    pub x: f32,
    pub y: f32,
}

fn encode(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn connection_dir(config: &Path, owner: &str) -> PathBuf {
    config
        .join("diagram-positions")
        .join(format!("connection-{}", encode(owner)))
}

pub(super) fn layout_path(config: &Path, owner: &str, schema: &str, table: &str) -> PathBuf {
    let mut path = connection_dir(config, owner);
    for (label, value) in [("schema", schema), ("table", table)] {
        path.push(label);
        let encoded = encode(value);
        for start in (0..encoded.len()).step_by(128) {
            path.push(&encoded[start..(start + 128).min(encoded.len())]);
        }
    }
    path.join("positions.json")
}

pub(super) fn load(path: &Path) -> anyhow::Result<Vec<Position>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_LAYOUT_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_LAYOUT_BYTES,
        "Diagram layout exceeds storage limit"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn save(path: &Path, positions: &[Position]) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(positions)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_LAYOUT_BYTES,
        "Diagram layout exceeds storage limit"
    );
    let directory = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Missing diagram layout directory"))?;
    std::fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

pub(super) fn delete_connection(config: &Path, owner: &str) -> std::io::Result<()> {
    match std::fs::remove_dir_all(connection_dir(config, owner)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_positions_reopen_and_cleanup_is_connection_scoped() {
        let config = tempfile::tempdir().unwrap();
        let long_name = "表".repeat(64);
        let a = layout_path(config.path(), "../one", &long_name, &long_name);
        let b = layout_path(config.path(), "two", "schema", "table");
        let other_root = layout_path(config.path(), "../one", "schema", "other");
        let positions = [Position {
            schema: "schema".into(),
            table: "table".into(),
            x: 20.0,
            y: 50.0,
        }];
        assert!(load(&a).unwrap().is_empty());
        save(&a, &positions).unwrap();
        save(&b, &positions).unwrap();
        save(&other_root, &positions).unwrap();
        assert_eq!(load(&a).unwrap()[0].y, 50.0);
        delete_connection(config.path(), "../one").unwrap();
        assert!(load(&a).unwrap().is_empty());
        assert!(load(&other_root).unwrap().is_empty());
        assert_eq!(load(&b).unwrap()[0].x, 20.0);
    }
}
