use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use anyhow::{bail, Context, Result};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::slotted_page::SlottedPage;

#[derive(Debug, Clone, Copy)]
pub struct PageSpillLocation {
    pub file_offset: u64,
    pub compressed_len: u32,
    pub uncompressed_len: u32,
}

pub struct DiskSpiller {
    pub file_path: PathBuf,
    file: File,
    index: HashMap<u32, PageSpillLocation>,
    current_offset: u64,
}

impl DiskSpiller {
    pub fn new() -> Result<Self> {
        let dir = platform_hal::paths::ensure_spill_dir()
            .context("Failed to ensure spill directory for DiskSpiller")?;
        let filename = format!("query_{}.spill", Uuid::new_v4());
        let file_path = dir.join(filename);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file_path)
            .with_context(|| format!("Failed to create spill file at {}", file_path.display()))?;

        Ok(Self {
            file_path,
            file,
            index: HashMap::new(),
            current_offset: 0,
        })
    }

    pub fn is_page_spilled(&self, page_id: u32) -> bool {
        self.index.contains_key(&page_id)
    }

    pub fn spilled_page_count(&self) -> usize {
        self.index.len()
    }

    /// Spills a SlottedPage onto disk compressed with LZ4
    pub fn spill_page(&mut self, page: &SlottedPage) -> Result<()> {
        let uncompressed = page.serialize_to_bytes();
        let uncompressed_len = uncompressed.len() as u32;
        let compressed = lz4_flex::compress_prepend_size(&uncompressed);
        let compressed_len = compressed.len() as u32;

        self.file.seek(SeekFrom::Start(self.current_offset))?;
        self.file.write_all(&compressed)?;
        self.file.flush()?;

        self.index.insert(
            page.page_id,
            PageSpillLocation {
                file_offset: self.current_offset,
                compressed_len,
                uncompressed_len,
            },
        );

        self.current_offset += compressed_len as u64;
        debug!(
            "Spilled page {} to disk ({} -> {} bytes LZ4)",
            page.page_id, uncompressed_len, compressed_len
        );

        Ok(())
    }

    /// Reads a previously spilled page from disk and decompresses it back into memory
    pub fn read_page(&mut self, page_id: u32) -> Result<SlottedPage> {
        let loc = match self.index.get(&page_id) {
            Some(&loc) => loc,
            None => bail!("Page {} not found in spill index", page_id),
        };

        self.file.seek(SeekFrom::Start(loc.file_offset))?;
        let mut compressed_buf = vec![0u8; loc.compressed_len as usize];
        self.file.read_exact(&mut compressed_buf)?;

        let uncompressed = lz4_flex::decompress_size_prepended(&compressed_buf)
            .map_err(|e| anyhow::anyhow!("LZ4 decompression failed for page {}: {}", page_id, e))?;

        let page = SlottedPage::deserialize_from_bytes(&uncompressed)?;
        Ok(page)
    }
}

impl Drop for DiskSpiller {
    fn drop(&mut self) {
        if self.file_path.exists() {
            if let Err(e) = fs::remove_file(&self.file_path) {
                warn!("Failed to clean up spill file {}: {}", self.file_path.display(), e);
            } else {
                debug!("Cleaned up spill file {}", self.file_path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disk_spiller_roundtrip() {
        let mut spiller = DiskSpiller::new().unwrap();
        let path = spiller.file_path.clone();

        let mut page = SlottedPage::with_capacity(100, 2);
        page.try_append_row(vec![Some(&b"user_1"[..]), Some(&b"pass_123"[..])]);
        page.try_append_row(vec![Some(&b"user_2"[..]), None]);

        spiller.spill_page(&page).unwrap();
        assert!(spiller.is_page_spilled(100));

        let restored = spiller.read_page(100).unwrap();
        assert_eq!(restored.page_id, 100);
        assert_eq!(restored.row_count, 2);
        assert_eq!(restored.get_cell(0, 0), Some(&b"user_1"[..]));
        assert_eq!(restored.get_cell(1, 1), None);

        // Check cleanup on drop
        drop(spiller);
        assert!(!path.exists());
    }
}
