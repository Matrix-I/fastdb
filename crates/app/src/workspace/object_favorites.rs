use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_FAVORITES: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Favorite {
    pub owner: String,
    pub kind: String,
    pub schema: String,
    pub name: String,
    pub qualifier: String,
}

impl Favorite {
    pub(super) fn object_kind(&self) -> Option<db_wire::objects::ObjectKind> {
        match self.kind.as_str() {
            "Sequence" => Some(db_wire::objects::ObjectKind::Sequence),
            "Routine" => Some(db_wire::objects::ObjectKind::Routine),
            "Trigger" => Some(db_wire::objects::ObjectKind::Trigger),
            "Type" => Some(db_wire::objects::ObjectKind::Type),
            _ => None,
        }
    }
}

pub(super) fn read(path: &Path) -> anyhow::Result<Vec<Favorite>> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "Favorites file exceeds 1 MiB");
    let favorites: Vec<Favorite> = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(favorites.len() <= MAX_FAVORITES, "Too many favorites");
    Ok(favorites)
}

pub(super) fn toggle(path: &Path, favorite: Favorite) -> anyhow::Result<Vec<Favorite>> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid favorites path"))?;
    std::fs::create_dir_all(parent)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))?;
    lock.lock()?;
    let mut favorites = read(path)?;
    if let Some(index) = favorites.iter().position(|f| f == &favorite) {
        favorites.remove(index);
    } else {
        anyhow::ensure!(favorites.len() < MAX_FAVORITES, "Favorites limit reached");
        favorites.push(favorite);
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid favorites path"))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let bytes = serde_json::to_vec_pretty(&favorites)?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "Favorites file exceeds 1 MiB");
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(favorites)
}

#[cfg(test)]
mod tests {
    use super::{Favorite, read, toggle};
    use db_wire::objects::ObjectKind;

    #[test]
    fn favorites_roundtrip_and_keep_owners_and_overloads_separate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("favorites.json");
        assert!(read(&path).unwrap().is_empty());
        let f = Favorite {
            owner: "one".into(),
            kind: "Routine".into(),
            schema: "s".into(),
            name: "f".into(),
            qualifier: "integer".into(),
        };
        assert_eq!(f.object_kind(), Some(ObjectKind::Routine));
        assert_eq!(toggle(&path, f.clone()).unwrap(), vec![f.clone()]);
        let other = Favorite {
            owner: "two".into(),
            ..f.clone()
        };
        assert_eq!(toggle(&path, other.clone()).unwrap().len(), 2);
        assert_eq!(toggle(&path, f).unwrap(), vec![other]);

        std::fs::write(&path, b"").unwrap();
        assert!(read(&path).unwrap().is_empty());

        std::fs::write(&path, b"invalid").unwrap();
        assert!(read(&path).is_err());
        assert!(
            toggle(
                &path,
                Favorite {
                    owner: "a".into(),
                    kind: "Type".into(),
                    schema: "s".into(),
                    name: "t".into(),
                    qualifier: "e".into()
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid");

        let unknown = Favorite {
            owner: "a".into(),
            kind: "UnknownKind".into(),
            schema: "s".into(),
            name: "n".into(),
            qualifier: "".into(),
        };
        assert_eq!(unknown.object_kind(), None);
    }
}
