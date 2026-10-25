use anyhow::Result;
use gpui::{AssetSource, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../assets"]
pub struct Assets;

pub struct FastDbAssetSource;

impl AssetSource for FastDbAssetSource {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match Assets::get(path) {
            Some(file) => Ok(Some(file.data)),
            None => Ok(None),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut list = Vec::new();
        for file in Assets::iter() {
            if file.starts_with(path) {
                list.push(SharedString::from(file.to_string()));
            }
        }
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dbeaver_icons_loadable() {
        let source = FastDbAssetSource;
        assert!(source.load("icons/folder_database.svg").unwrap().is_some());
        assert!(source.load("icons/database.svg").unwrap().is_some());
        assert!(source.load("icons/folder_schema.svg").unwrap().is_some());
        assert!(source.load("icons/schema.svg").unwrap().is_some());
        assert!(source.load("icons/folder_table.svg").unwrap().is_some());
        assert!(source.load("icons/table.svg").unwrap().is_some());
        assert!(source.load("icons/columns.svg").unwrap().is_some());
        assert!(source.load("icons/column.svg").unwrap().is_some());
        assert!(source.load("icons/folder_constraint.svg").unwrap().is_some());
        assert!(source.load("icons/constraint.svg").unwrap().is_some());
        assert!(source.load("icons/folder.svg").unwrap().is_some());
        assert!(source.load("icons/foreign_key.svg").unwrap().is_some());
        assert!(source.load("icons/table_index.svg").unwrap().is_some());
        assert!(source.load("icons/database_navigator.svg").unwrap().is_some());
        assert!(source.load("icons/connect.svg").unwrap().is_some());
        assert!(source.load("icons/disconnect.svg").unwrap().is_some());
        assert!(source.load("icons/sql_exec.svg").unwrap().is_some());
        assert!(source.load("icons/sql_script.svg").unwrap().is_some());
        assert!(source.load("icons/grid.svg").unwrap().is_some());
        assert!(source.load("icons/configuration.svg").unwrap().is_some());
        assert!(source.load("icons/sql_validate.svg").unwrap().is_some());
        assert!(source.load("icons/function.svg").unwrap().is_some());
        assert!(source.load("icons/sql.svg").unwrap().is_some());
        assert!(source.load("icons/postgresql.png").unwrap().is_some());
        assert!(source.load("icons/mysql.png").unwrap().is_some());
        assert!(source.load("icons/sqlite.png").unwrap().is_some());
        assert!(source.load("icons/success_ovr.png").unwrap().is_some());
    }
}
