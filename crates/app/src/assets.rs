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
