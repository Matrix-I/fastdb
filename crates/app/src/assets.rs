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
    fn provider_card_logos_are_scalable_vector_artwork() {
        let source = FastDbAssetSource;
        for path in [
            "icons/postgresql.svg",
            "icons/mysql.svg",
            "icons/sqlite.svg",
        ] {
            let data = source.load(path).unwrap().unwrap();
            assert!(
                !data.starts_with(b"\x89PNG"),
                "{path}: raster logo cannot scale cleanly from 16px to 88px Retina cards"
            );
            let svg = std::str::from_utf8(&data).unwrap();
            assert!(
                svg.contains("<svg") && svg.contains("viewBox="),
                "{path}: missing scalable canvas"
            );
            assert!(svg.contains("<path"), "{path}: missing vector artwork");
            assert!(
                !svg.contains("<image"),
                "{path}: embedded bitmap would still pixelate"
            );
        }
    }

    #[test]
    fn rail_labels_rotate_readable_horizontal_glyphs_without_reflection() {
        let source = FastDbAssetSource;
        for (name, transform, first_point) in [
            ("grid", "matrix(0 -1 1 0 0 23)", "M 6.00 8.57"),
            ("text", "matrix(0 -1 1 0 0 24)", "M 7.12 1.00"),
            ("record", "matrix(0 -1 1 0 0 37)", "M 4.17 5.59"),
            ("panels", "matrix(0 1 -1 0 10 0)", "M 4.17 5.84"),
        ] {
            let data = source
                .load(&format!("icons/label_{name}.svg"))
                .unwrap()
                .unwrap();
            let svg = std::str::from_utf8(&data).unwrap();
            assert!(
                svg.contains(transform),
                "{name}: expected rotation without reflection"
            );
            assert!(
                svg.contains(first_point),
                "{name}: glyph coordinates must be horizontal before rotation"
            );
        }
    }

    #[test]
    fn panels_icon_combines_grid_and_configuration_artwork() {
        let source = FastDbAssetSource;
        let data = source
            .load("icons/panels.svg")
            .unwrap()
            .expect("Panels icon must be embedded");
        let icon = std::str::from_utf8(&data).unwrap();
        for original in ["icons/grid.svg", "icons/configuration.svg"] {
            let data = source.load(original).unwrap().unwrap();
            let svg = std::str::from_utf8(&data).unwrap();
            for path in svg.split("<path d=\"").skip(1) {
                let outline = path.split('"').next().unwrap();
                assert!(icon.contains(outline), "missing artwork from {original}");
            }
        }
    }

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
        assert!(
            source
                .load("icons/folder_constraint.svg")
                .unwrap()
                .is_some()
        );
        assert!(source.load("icons/constraint.svg").unwrap().is_some());
        assert!(source.load("icons/folder.svg").unwrap().is_some());
        assert!(source.load("icons/foreign_key.svg").unwrap().is_some());
        assert!(source.load("icons/table_index.svg").unwrap().is_some());
        assert!(
            source
                .load("icons/database_navigator.svg")
                .unwrap()
                .is_some()
        );
        assert!(source.load("icons/connect.svg").unwrap().is_some());
        assert!(source.load("icons/disconnect.svg").unwrap().is_some());
        assert!(source.load("icons/edit.svg").unwrap().is_some());
        assert!(source.load("icons/delete.svg").unwrap().is_some());
        assert!(source.load("icons/refresh.svg").unwrap().is_some());
        assert!(source.load("icons/sql_exec.svg").unwrap().is_some());
        assert!(source.load("icons/sql_script.svg").unwrap().is_some());
        assert!(source.load("icons/grid.svg").unwrap().is_some());
        assert!(source.load("icons/configuration.svg").unwrap().is_some());
        assert!(source.load("icons/sql_validate.svg").unwrap().is_some());
        assert!(source.load("icons/function.svg").unwrap().is_some());
        assert!(source.load("icons/sql.svg").unwrap().is_some());
        assert!(source.load("icons/postgresql.svg").unwrap().is_some());
        assert!(source.load("icons/mysql.svg").unwrap().is_some());
        assert!(source.load("icons/sqlite.svg").unwrap().is_some());
        assert!(source.load("icons/label_grid.svg").unwrap().is_some());
        assert!(source.load("icons/label_text.svg").unwrap().is_some());
        assert!(source.load("icons/label_record.svg").unwrap().is_some());
        assert!(source.load("icons/label_panels.svg").unwrap().is_some());
        assert!(source.load("icons/success_ovr.png").unwrap().is_some());
    }
}
