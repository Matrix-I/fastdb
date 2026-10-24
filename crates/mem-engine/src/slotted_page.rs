use anyhow::{bail, Result};

pub const PAGE_SIZE: usize = 64 * 1024; // 64 KB (Standard 16KB Apple Silicon & 4KB Linux Alignment)

#[derive(Debug, Clone)]
pub struct SlottedPage {
    pub page_id: u32,
    pub num_cols: u16,
    pub row_count: u32,
    /// Start position of each row inside the payload array
    pub row_offsets: Vec<u32>,
    /// Contiguous flat byte buffer containing all row data
    pub payload: Vec<u8>,
}

impl SlottedPage {
    pub fn with_capacity(page_id: u32, num_cols: u16) -> Self {
        Self {
            page_id,
            num_cols,
            row_count: 0,
            row_offsets: Vec::with_capacity(512),
            payload: Vec::with_capacity(PAGE_SIZE),
        }
    }

    /// Access the raw byte slice of a cell in O(1) time without memory allocation.
    /// Returns None if cell is SQL NULL or coordinates are out of bounds.
    #[inline(always)]
    pub fn get_cell(&self, row_idx: usize, col_idx: usize) -> Option<&[u8]> {
        if col_idx >= self.num_cols as usize {
            return None;
        }

        let row_start = *self.row_offsets.get(row_idx)? as usize;
        let bitmap_len = (self.num_cols as usize + 7) / 8;

        if row_start + bitmap_len > self.payload.len() {
            return None;
        }

        // 1. Check Null Bitmap
        let null_byte = self.payload[row_start + (col_idx / 8)];
        if (null_byte & (1 << (col_idx % 8))) != 0 {
            return None; // SQL NULL
        }

        // 2. Compute coordinates using the cell offsets table
        let offsets_start = row_start + bitmap_len;
        let data_start_base = offsets_start + (self.num_cols as usize * 4);

        let cell_start = if col_idx == 0 {
            0
        } else {
            let prev_offset_pos = offsets_start + (col_idx - 1) * 4;
            u32::from_le_bytes(
                self.payload[prev_offset_pos..prev_offset_pos + 4].try_into().ok()?
            ) as usize
        };

        let curr_offset_pos = offsets_start + col_idx * 4;
        let cell_end = u32::from_le_bytes(
            self.payload[curr_offset_pos..curr_offset_pos + 4].try_into().ok()?
        ) as usize;

        if data_start_base + cell_end > self.payload.len() || cell_start > cell_end {
            return None;
        }

        Some(&self.payload[data_start_base + cell_start..data_start_base + cell_end])
    }

    /// Estimates the payload bytes needed for a row with given cell values
    pub fn required_row_bytes<'a, I>(num_cols: u16, cells: I) -> usize
    where
        I: IntoIterator<Item = Option<&'a [u8]>>,
    {
        let bitmap_len = (num_cols as usize + 7) / 8;
        let offsets_len = num_cols as usize * 4;
        let data_len: usize = cells.into_iter().filter_map(|c| c.map(|b| b.len())).sum();
        bitmap_len + offsets_len + data_len
    }

    /// Appends a row to the page. Returns true if successfully inserted.
    /// If inserting this row exceeds PAGE_SIZE and row_count > 0, returns false without modifying the page.
    pub fn try_append_row<I, C>(&mut self, cells: I) -> bool
    where
        I: IntoIterator<Item = Option<C>>,
        C: AsRef<[u8]>,
    {
        let cell_vec: Vec<Option<C>> = cells.into_iter().collect();
        let num_cols = self.num_cols as usize;
        let bitmap_len = (num_cols + 7) / 8;
        let offsets_len = num_cols * 4;

        let mut data_len = 0;
        for c in cell_vec.iter().take(num_cols) {
            if let Some(bytes) = c {
                data_len += bytes.as_ref().len();
            }
        }

        let needed_bytes = bitmap_len + offsets_len + data_len;
        let total_est = self.payload.len() + needed_bytes + (self.row_offsets.len() + 1) * 4;

        // If not empty and exceeds PAGE_SIZE, signal caller to spill/switch page
        if self.row_count > 0 && total_est > PAGE_SIZE {
            return false;
        }

        let row_start = self.payload.len() as u32;
        self.row_offsets.push(row_start);

        // 1. Build Null Bitmap
        let mut null_bitmap = vec![0u8; bitmap_len];
        for i in 0..num_cols {
            let is_null = match cell_vec.get(i) {
                Some(Some(_)) => false,
                _ => true,
            };
            if is_null {
                null_bitmap[i / 8] |= 1 << (i % 8);
            }
        }
        self.payload.extend_from_slice(&null_bitmap);

        // 2. Build Cell Offsets & Data
        let mut curr_offset: u32 = 0;
        let mut cell_offsets = Vec::with_capacity(offsets_len);
        let mut data_bytes = Vec::with_capacity(data_len);

        for i in 0..num_cols {
            if let Some(Some(bytes)) = cell_vec.get(i) {
                let slice = bytes.as_ref();
                curr_offset += slice.len() as u32;
                data_bytes.extend_from_slice(slice);
            }
            cell_offsets.extend_from_slice(&curr_offset.to_le_bytes());
        }

        self.payload.extend_from_slice(&cell_offsets);
        self.payload.extend_from_slice(&data_bytes);
        self.row_count += 1;

        true
    }

    /// Serializes this SlottedPage into a compact binary format for disk spilling
    pub fn serialize_to_bytes(&self) -> Vec<u8> {
        let offsets_len = self.row_offsets.len() * 4;
        let total_len = 4 + 2 + 4 + 4 + offsets_len + 4 + self.payload.len();
        let mut buf = Vec::with_capacity(total_len);

        buf.extend_from_slice(&self.page_id.to_le_bytes());
        buf.extend_from_slice(&self.num_cols.to_le_bytes());
        buf.extend_from_slice(&self.row_count.to_le_bytes());

        buf.extend_from_slice(&(self.row_offsets.len() as u32).to_le_bytes());
        for &offset in &self.row_offsets {
            buf.extend_from_slice(&offset.to_le_bytes());
        }

        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.payload);

        buf
    }

    /// Deserializes a SlottedPage from bytes
    pub fn deserialize_from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 18 {
            bail!("Invalid SlottedPage payload: too short ({} bytes)", bytes.len());
        }

        let page_id = u32::from_le_bytes(bytes[0..4].try_into()?);
        let num_cols = u16::from_le_bytes(bytes[4..6].try_into()?);
        let row_count = u32::from_le_bytes(bytes[6..10].try_into()?);

        let num_offsets = u32::from_le_bytes(bytes[10..14].try_into()?) as usize;
        let offsets_end = 14 + (num_offsets * 4);
        if bytes.len() < offsets_end + 4 {
            bail!("Truncated row offsets in SlottedPage binary data");
        }

        let mut row_offsets = Vec::with_capacity(num_offsets);
        for i in 0..num_offsets {
            let start = 14 + (i * 4);
            let val = u32::from_le_bytes(bytes[start..start + 4].try_into()?);
            row_offsets.push(val);
        }

        let payload_len = u32::from_le_bytes(bytes[offsets_end..offsets_end + 4].try_into()?) as usize;
        let payload_start = offsets_end + 4;
        let payload_end = payload_start + payload_len;

        if bytes.len() < payload_end {
            bail!("Truncated payload in SlottedPage binary data");
        }

        let payload = bytes[payload_start..payload_end].to_vec();

        Ok(Self {
            page_id,
            num_cols,
            row_count,
            row_offsets,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_append_and_get_cell() {
        let mut page = SlottedPage::with_capacity(1, 3);
        
        let row1 = vec![Some(&b"alice"[..]), None, Some(&b"123"[..])];
        let ok = page.try_append_row(row1);
        assert!(ok);

        let row2 = vec![None, Some(&b"bob"[..]), Some(&b"456"[..])];
        let ok2 = page.try_append_row(row2);
        assert!(ok2);

        assert_eq!(page.row_count, 2);

        // Row 1 checks
        assert_eq!(page.get_cell(0, 0), Some(&b"alice"[..]));
        assert_eq!(page.get_cell(0, 1), None);
        assert_eq!(page.get_cell(0, 2), Some(&b"123"[..]));

        // Row 2 checks
        assert_eq!(page.get_cell(1, 0), None);
        assert_eq!(page.get_cell(1, 1), Some(&b"bob"[..]));
        assert_eq!(page.get_cell(1, 2), Some(&b"456"[..]));

        // Out of bounds
        assert_eq!(page.get_cell(2, 0), None);
        assert_eq!(page.get_cell(0, 3), None);
    }

    #[test]
    fn test_page_serialization_roundtrip() {
        let mut page = SlottedPage::with_capacity(42, 2);
        page.try_append_row(vec![Some(&b"key"[..]), Some(&b"value"[..])]);
        page.try_append_row(vec![None, Some(&b"only_val"[..])]);

        let bytes = page.serialize_to_bytes();
        let restored = SlottedPage::deserialize_from_bytes(&bytes).unwrap();

        assert_eq!(restored.page_id, 42);
        assert_eq!(restored.num_cols, 2);
        assert_eq!(restored.row_count, 2);
        assert_eq!(restored.get_cell(0, 0), Some(&b"key"[..]));
        assert_eq!(restored.get_cell(0, 1), Some(&b"value"[..]));
        assert_eq!(restored.get_cell(1, 0), None);
        assert_eq!(restored.get_cell(1, 1), Some(&b"only_val"[..]));
    }
}
