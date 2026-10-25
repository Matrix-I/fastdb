use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use anyhow::{bail, Result};
use lru::LruCache;
use parking_lot::{Mutex, RwLock};

use crate::slotted_page::SlottedPage;
use crate::spiller::DiskSpiller;
use crate::types::ColumnMeta;

pub const DEFAULT_MAX_RESIDENT_PAGES: usize = 1024; // 1,024 pages x 64KB ~= 64 MB RAM

pub struct QueryResultPager {
    pub columns: Vec<ColumnMeta>,
    pub num_cols: u16,
    total_rows: AtomicU64,
    max_resident_pages: usize,
    /// LRU Cache for cached slotted pages
    cache: Mutex<LruCache<u32, Arc<SlottedPage>>>,
    /// Disk spiller for offloading cold pages
    spiller: Mutex<DiskSpiller>,
    /// The current active page receiving incoming rows. Readers share the Arc;
    /// the writer copies on write only while a reader still holds a snapshot.
    active_page: Mutex<Arc<SlottedPage>>,
    /// Global row start index for each page: page_id -> starting global row
    page_row_starts: RwLock<Vec<u64>>,
}

impl QueryResultPager {
    pub fn new(columns: Vec<ColumnMeta>) -> Result<Self> {
        let num_cols = columns.len() as u16;
        let spiller = DiskSpiller::new()?;
        let cap = NonZeroUsize::new(DEFAULT_MAX_RESIDENT_PAGES).unwrap();
        let active_page = Arc::new(SlottedPage::with_capacity(0, num_cols));

        Ok(Self {
            columns,
            num_cols,
            total_rows: AtomicU64::new(0),
            max_resident_pages: DEFAULT_MAX_RESIDENT_PAGES,
            cache: Mutex::new(LruCache::new(cap)),
            spiller: Mutex::new(spiller),
            active_page: Mutex::new(active_page),
            page_row_starts: RwLock::new(vec![0]),
        })
    }

    pub fn with_max_resident_pages(mut self, max: usize) -> Self {
        self.max_resident_pages = max.max(2);
        let cap = NonZeroUsize::new(self.max_resident_pages).unwrap();
        *self.cache.lock() = LruCache::new(cap);
        self
    }

    pub fn total_rows(&self) -> u64 {
        self.total_rows.load(Ordering::Acquire)
    }

    pub fn num_cols(&self) -> u16 {
        self.num_cols
    }

    /// Append a single row of cells into the pager.
    /// Automatically manages 64KB page boundaries and LRU disk spilling.
    /// Returns (new_total_rows, page_rollover_occurred).
    pub fn append_row<I, C>(&self, cells: I) -> Result<(u64, bool)>
    where
        I: IntoIterator<Item = Option<C>>,
        C: AsRef<[u8]>,
    {
        let cells_vec: Vec<Option<C>> = cells.into_iter().collect();
        let mut active = self.active_page.lock();
        let mut rollover = false;

        let row_slices: Vec<Option<&[u8]>> = cells_vec.iter().map(|c| c.as_ref().map(|b| b.as_ref())).collect();
        if !Arc::make_mut(&mut *active).try_append_row(row_slices.iter().copied()) {
            // Active page is full. Seal it and start a new page.
            rollover = true;
            let current_id = active.page_id;
            let sealed_page = std::mem::replace(
                &mut *active,
                Arc::new(SlottedPage::with_capacity(current_id + 1, self.num_cols)),
            );

            self.insert_sealed_page(current_id, sealed_page)?;

            // Register start of new page
            let current_total = self.total_rows.load(Ordering::Relaxed);
            self.page_row_starts.write().push(current_total);

            // Now append into the new active page
            let ok = Arc::make_mut(&mut *active).try_append_row(row_slices);
            if !ok {
                bail!("Row is larger than maximum page capacity (64KB)");
            }
        }

        let new_total = self.total_rows.fetch_add(1, Ordering::Release) + 1;
        Ok((new_total, rollover))
    }

    fn insert_sealed_page(&self, page_id: u32, page: Arc<SlottedPage>) -> Result<()> {
        let mut cache = self.cache.lock();

        if cache.len() >= self.max_resident_pages {
            // Evict LRU page and spill to disk
            if let Some((evicted_id, evicted_page)) = cache.pop_lru() {
                let mut spiller = self.spiller.lock();
                if !spiller.is_page_spilled(evicted_id) {
                    spiller.spill_page(&evicted_page)?;
                }
            }
        }

        cache.put(page_id, page);
        Ok(())
    }

    /// Retrieve an Arc reference to the specified SlottedPage.
    /// Handles transparent LZ4 retrieval from disk if spilled.
    pub fn get_page(&self, page_id: u32) -> Result<Arc<SlottedPage>> {
        // 1. Check if it's the active page
        {
            let active = self.active_page.lock();
            if active.page_id == page_id {
                return Ok(Arc::clone(&active));
            }
        }

        // 2. Check LRU Cache
        {
            let mut cache = self.cache.lock();
            if let Some(page) = cache.get(&page_id) {
                return Ok(Arc::clone(page));
            }
        }

        // 3. Cache Miss: Read from Disk Spiller
        let page = {
            let mut spiller = self.spiller.lock();
            spiller.read_page(page_id)?
        };

        let arc_page = Arc::new(page);

        // Put back into LRU cache
        {
            let mut cache = self.cache.lock();
            if cache.len() >= self.max_resident_pages {
                if let Some((evicted_id, evicted_page)) = cache.pop_lru() {
                    let mut spiller = self.spiller.lock();
                    if !spiller.is_page_spilled(evicted_id) {
                        spiller.spill_page(&evicted_page)?;
                    }
                }
            }
            cache.put(page_id, Arc::clone(&arc_page));
        }

        Ok(arc_page)
    }

    /// Find the page ID and local row index within that page for a global row index
    pub fn locate_row(&self, global_row_idx: u64) -> Option<(u32, usize)> {
        let starts = self.page_row_starts.read();
        if starts.is_empty() || global_row_idx >= self.total_rows() {
            return None;
        }

        // Binary search the largest page_start <= global_row_idx
        let page_idx = match starts.binary_search(&global_row_idx) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };

        let page_start = starts[page_idx];
        let local_row = (global_row_idx - page_start) as usize;
        Some((page_idx as u32, local_row))
    }

    /// Get cell content copied as Vec<u8> (or None if NULL/out of bounds)
    pub fn get_cell(&self, global_row_idx: u64, col_idx: usize) -> Option<Vec<u8>> {
        let (page_id, local_row) = self.locate_row(global_row_idx)?;
        let page = self.get_page(page_id).ok()?;
        page.get_cell(local_row, col_idx).map(|s| s.to_vec())
    }

    /// Zero-copy access to a cell using a visitor callback closure
    pub fn with_cell<F, R>(&self, global_row_idx: u64, col_idx: usize, f: F) -> Option<R>
    where
        F: FnOnce(&[u8]) -> R,
    {
        let (page_id, local_row) = self.locate_row(global_row_idx)?;
        let page = self.get_page(page_id).ok()?;
        let slice = page.get_cell(local_row, col_idx)?;
        Some(f(slice))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DataType;

    #[test]
    fn test_active_page_reads_share_one_allocation() {
        let pager = QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int8)]).unwrap();
        for i in 0..10u32 {
            pager.append_row(vec![Some(i.to_string().into_bytes())]).unwrap();
        }

        // Repeated reads of the active page must not copy the 64KB page
        let first = pager.get_page(0).unwrap();
        let second = pager.get_page(0).unwrap();
        assert!(Arc::ptr_eq(&first, &second));

        // A reader's snapshot stays stable while the writer keeps appending (copy-on-write)
        pager.append_row(vec![Some(b"10".to_vec())]).unwrap();
        assert_eq!(first.row_count, 10);
        assert_eq!(pager.get_page(0).unwrap().row_count, 11);
        assert_eq!(pager.get_cell(10, 0).unwrap(), b"10");
    }

    #[test]
    fn test_pager_spill_and_random_access() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("name", DataType::Text),
        ];

        // Use a tiny cache size of 3 pages to force aggressive disk spilling
        let pager = QueryResultPager::new(cols)
            .unwrap()
            .with_max_resident_pages(3);

        let num_rows = 5_000u64;
        for i in 0..num_rows {
            let id_str = i.to_string();
            let name_str = format!("user_{:06}", i);
            let row = vec![Some(id_str.as_bytes()), Some(name_str.as_bytes())];
            pager.append_row(row).unwrap();
        }

        assert_eq!(pager.total_rows(), num_rows);

        // Verify random access across both memory and spilled pages
        let check_indices = [0, 1, 100, 500, 1500, 3000, 4999];
        for &idx in &check_indices {
            let id_bytes = pager.get_cell(idx, 0).unwrap();
            let name_bytes = pager.get_cell(idx, 1).unwrap();

            assert_eq!(std::str::from_utf8(&id_bytes).unwrap(), idx.to_string());
            assert_eq!(
                std::str::from_utf8(&name_bytes).unwrap(),
                format!("user_{:06}", idx)
            );
        }
    }
}
