# FASTDB — TÀI LIỆU KIẾN TRÚC & KẾ HOẠCH TRIỂN KHAI CHI TIẾT
> **Mục tiêu:** Database Client thế hệ mới thay thế DBeaver — Không JVM/JDBC, Pure Rust + GPUI (120 FPS), Zero-Copy Memory, hỗ trợ Native cho **macOS** & **Linux** từ ngày đầu.

---

## 1. CHỈ SỐ MỤC TIÊU (BENCHMARK TARGETS)

| Chỉ số | DBeaver (Java/JDBC) | FastDB (Rust/GPUI) |
| :--- | :--- | :--- |
| **Thời gian khởi động (Cold Start)** | 4.5s – 12s | **< 120ms** |
| **RAM khi mở app (Idle)** | 600 MB – 1.2 GB | **< 35 MB** |
| **RAM khi query 1,000,000 dòng × 20 cột** | 2.5 GB – OutOfMemory | **ố định ~80 MB (Tràn đĩa tự động)** |
| **Time-to-First-Row (Hiển thị lô đầu tiên)** | 300ms – 2,000ms | **< 10ms** |
| **Tốc độ cuộn bảng dữ liệu (Scrolling)** | 15 – 45 FPS (Giật khi GC chạy) | **120 FPS (Vẽ trực tiếp bằng Metal/Vulkan)** |
| **Kích thước bộ cài đặt** | ~130 MB (Kèm JRE) | **~12 MB (.dmg / .AppImage)** |

---

## 2. CẤU TRÚC THƯ MỤC DỰ ÁN (FILE-LEVEL WORKSPACE TREE)

```text
fastdb/
├── Cargo.toml                        # Cấu hình Workspace & Dependencies dùng chung
├── rust-toolchain.toml               # Khóa phiên bản Rust ổn định (ví dụ: 1.85+)
├── assets/
│   ├── fonts/                        # Nhúng sẵn JetBrainsMono-Regular.ttf & Inter-Regular.ttf
│   └── icons/                        # SVG icons cho bảng, cột, khóa chính, khóa ngoại
├── crates/
│   ├── app/                          # [Binary] Entrypoint khởi chạy ứng dụng
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs               # Khởi tạo Tokio Runtime + GPUI App + Window
│   │       └── workspace.rs          # Bố cục chính: Sidebar + TabBar + Main Dock + StatusBar
│   │
│   ├── platform-hal/                 # [Lib] Lớp trừu tượng hệ điều hành (macOS & Linux)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── paths.rs              # Đường dẫn Config, Cache và thư mục Spill-to-disk
│   │       ├── keymap.rs             # Chuẩn hóa phím tắt (Cmd trên macOS, Ctrl trên Linux)
│   │       ├── keychain.rs           # macOS Security.framework & Linux Secret Service (DBus)
│   │       └── socket.rs             # Tự động dò tìm Unix Domain Socket (/tmp vs /var/run)
│   │
│   ├── mem-engine/                   # [Lib] Quản lý bộ nhớ phẳng Zero-Copy & Mmap Disk Spill
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs              # Định nghĩa DataType, ColumnMeta, RawCellSlice
│   │       ├── slotted_page.rs       # Trang bộ nhớ 64KB (16KB-aligned cho cả Apple Silicon & Linux)
│   │       ├── pager.rs              # LRU Cache (1,024 trang nóng = 64MB) + Truy xuất O(1)
│   │       └── spiller.rs            # Nén LZ4 + Ghi/Đọc file tạm qua memmap2
│   │
│   ├── db-wire/                      # [Lib] Giao thức kết nối Database nhị phân (Không JDBC)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── traits.rs             # Trait DatabaseDriver, SessionHandle, CancelToken
│   │       ├── ssh_tunnel.rs         # SSH Jump Host / Bastion tích hợp sẵn qua russh
│   │       ├── postgres/
│   │       │   ├── mod.rs            # Kết nối TCP / Unix Socket & Out-of-Band Cancel
│   │       │   └── binary_decoder.rs # Đọc Postgres Binary Wire ghi thẳng vào SlottedPage
│   │       ├── mysql/
│   │       │   └── mod.rs            # MySQL Binary Protocol streaming
│   │       └── sqlite/
│   │           └── mod.rs            # SQLite C-FFI trực tiếp
│   │
│   ├── sql-intel/                    # [Lib] Phân tích cú pháp SQL & Gợi ý code tốc độ cao
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── parser.rs             # Tree-sitter SQL: tách câu lệnh tại con trỏ, nhận diện CTE/Join
│   │       ├── schema_cache.rs       # SQLite In-Memory lưu metadata bảng, cột, index, FK
│   │       └── completion.rs         # Engine gợi ý tên bảng/cột/alias dưới 2ms
│   │
│   ├── ui-core/                      # [Lib] Thành phần giao diện cơ sở
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── theme.rs              # Bảng màu Dark/Light, kích thước chuẩn
│   │       └── actions.rs            # Định nghĩa GPUI Actions toàn cục
│   │
│   ├── ui-editor/                    # [Lib] Trình soạn thảo SQL 120 FPS
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── buffer.rs             # Ropey Text Buffer + Undo/Redo history
│   │       └── editor_view.rs        # GPUI Element vẽ code, highlight AST, multi-cursor
│   │
│   └── ui-grid/                      # [Lib] Bảng dữ liệu ảo hóa 2 chiều (2D Virtual Grid)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── grid_element.rs       # Custom GPUI Element chỉ vẽ các ô trong Viewport
│           ├── selection.rs          # Chọn vùng ô/dòng/cột, copy CSV/JSON/SQL Insert
│           └── changeset.rs          # Lưu các ô sửa trực tiếp (Inline Edit) & sinh câu lệnh UPDATE
│
└── ci/
    ├── build-macos.sh                # Đóng gói .app & .dmg (aarch64-apple-darwin)
    └── build-linux.sh                # Đóng gói .deb & .AppImage (x86_64-unknown-linux-gnu)
```

---

## 3. CẤU HÌNH `Cargo.toml` GỐC (ROOT WORKSPACE)

```toml
[workspace]
resolver = "2"
members = [
    "crates/app",
    "crates/platform-hal",
    "crates/mem-engine",
    "crates/db-wire",
    "crates/sql-intel",
    "crates/ui-core",
    "crates/ui-editor",
    "crates/ui-grid",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"

[workspace.dependencies]
# UI Framework (Zed GPUI)
gpui = { git = "[https://github.com/zed-industries/zed](https://github.com/zed-industries/zed)", package = "gpui" }

# Async Runtime & Channels
tokio = { version = "1.43", features = ["rt-multi-thread", "net", "io-util", "time", "sync", "fs"] }
flume = "0.11"
futures = "0.3"
async-trait = "0.1"

# Memory, Concurrency & Compression
bytes = "1.9"
memmap2 = "0.9"
lz4_flex = { version = "0.11", default-features = false, features = ["safe-encode", "safe-decode"] }
parking_lot = "0.12"
lru = "0.12"
smallvec = "1.13"

# Database Drivers & SSH
tokio-postgres = { version = "0.7", features = ["runtime"] }
postgres-types = "0.2"
mysql_async = "0.34"
rusqlite = { version = "0.32", features = ["bundled"] }
russh = "0.46"
russh-keys = "0.46"

# Text Buffer & SQL AST
ropey = "1.6"
tree-sitter = "0.24"
tree-sitter-sequel = "0.3"

# OS Integration (macOS & Linux)
directories = "5.0"
keyring = { version = "3.6", features = ["apple-native", "sync-secret-service", "crypto-rust"] }

# Error & Logging
anyhow = "1.0"
thiserror = "2.0"
tracing = "0.1"
tracing-subscriber = "0.3"

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
strip = true
```

---

## 4. THIẾT KẾ KỸ THUẬT CHI TIẾT TỪNG MODULE CỐT LÕI

### 4.1. Lớp Trừu tượng Hệ điều hành (`crates/platform-hal`)

Giải quyết triệt để mọi khác biệt giữa **macOS** và **Linux** ngay từ tầng thấp nhất:

* **Chiến lược thư mục Spill-to-Disk (`paths.rs`):**
  * **Cảnh báo quan trọng trên Linux:** Không bao giờ ghi file tràn đĩa (`mmap`) vào `/tmp` trên Linux vì đa số bản phân phối Linux hiện đại gắn `/tmp` vào `tmpfs` (nằm trực tiếp trên RAM). Phải dùng `/var/tmp/fastdb` trên Linux và `std::env::temp_dir()` (`$TMPDIR`) trên macOS.
* **Tự động phát hiện Unix Domain Socket (`socket.rs`):**

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub enum DbKind {
    Postgres,
    MySql,
}

/// Tự động tìm Unix Socket trên macOS và Linux khi host là localhost/127.0.0.1
pub fn detect_local_unix_socket(kind: DbKind, host: &str, port: u16) -> Option<PathBuf> {
    if host != "localhost" && host != "127.0.0.1" {
        return None;
    }

    let candidates: Vec<String> = match kind {
        DbKind::Postgres => vec![
            format!("/tmp/.s.PGSQL.{port}"),                // macOS (Homebrew / Postgres.app)
            format!("/var/run/postgresql/.s.PGSQL.{port}"), // Linux (Debian/Ubuntu)
            format!("/run/postgresql/.s.PGSQL.{port}"),     // Linux (Arch/Fedora)
        ],
        DbKind::MySql => vec![
            "/tmp/mysql.sock".to_string(),                  // macOS
            "/var/run/mysqld/mysqld.sock".to_string(),      // Linux (Ubuntu/Debian)
            "/var/lib/mysql/mysql.sock".to_string(),        // Linux (RHEL/Fedora)
        ],
    };

    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}
```

* **Chuẩn hóa Phím tắt (`keymap.rs`):**
  * Sử dụng modifier `secondary` của GPUI cho tất cả phím tắt chính:
    * `secondary-enter`: Chạy câu lệnh SQL tại con trỏ (`Cmd+Enter` trên Mac, `Ctrl+Enter` trên Linux).
    * `secondary-s`: Commit các thay đổi Inline Edit xuống DB.
    * `secondary-t` / `secondary-w`: Mở / Đóng tab SQL.
    * `secondary-k`: Mở thanh Command Palette nhanh.

---

### 4.2. Bộ nhớ Zero-Copy & Tràn đĩa (`crates/mem-engine`)

#### A. Cấu trúc Byte của `SlottedPage` (64KB = 65,536 bytes)
* Kích thước **64KB** là bội số của **16KB** (trang nhớ Kernel của macOS Apple Silicon M1/M2/M3/M4) và **4KB** (trang nhớ Kernel của Linux), giúp ánh xạ `mmap` theo từng trang cực kỳ an toàn.
* **Không dùng `String` hay `Vec` cho từng ô:** Dữ liệu của mỗi dòng được ghi nối tiếp vào mảng `payload: Vec<u8>`.
* **Định dạng 1 dòng trong `payload`:**
  * `[Null Bitmap: ceil(num_cols / 8) bytes]`: Mỗi bit đại diện cho 1 cột (`1` = NULL, `0` = Có giá trị).
  * `[Cell Offsets: num_cols * 4 bytes (u32)]`: Vị trí kết thúc của từng cột trong dòng đó.
  * `[Raw Cell Bytes]`: Dữ liệu nhị phân thô nối tiếp nhau (`i32`, `i64`, `f64`, UTF-8 bytes...).

```rust
pub const PAGE_SIZE: usize = 64 * 1024; // 64 KB (Chuẩn 16KB/4KB OS Page Alignment)

pub struct SlottedPage {
    pub page_id: u32,
    pub num_cols: u16,
    pub row_count: u32,
    /// Vị trí bắt đầu của từng dòng trong mảng payload
    pub row_offsets: Vec<u32>,
    /// Vùng nhớ phẳng chứa toàn bộ dữ liệu của trang
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

    /// Truy xuất lát cắt byte của 1 ô trong thời gian O(1) mà không cấp phát bộ nhớ
    #[inline(always)]
    pub fn get_cell(&self, row_idx: usize, col_idx: usize) -> Option<&[u8]> {
        let row_start = *self.row_offsets.get(row_idx)? as usize;
        let bitmap_len = (self.num_cols as usize + 7) / 8;

        // 1. Kiểm tra Null Bitmap
        let null_byte = self.payload[row_start + (col_idx / 8)];
        if (null_byte & (1 << (col_idx % 8))) != 0 {
            return None; // Giá trị là SQL NULL
        }

        // 2. Tính tọa độ byte của ô thông qua bảng Cell Offset
        let offsets_start = row_start + bitmap_len;
        let data_start_base = offsets_start + (self.num_cols as usize * 4);

        let cell_start = if col_idx == 0 {
            0
        } else {
            let prev_offset_pos = offsets_start + (col_idx - 1) * 4;
            u32::from_le_bytes(
                self.payload[prev_offset_pos..prev_offset_pos + 4].try_into().unwrap()
            ) as usize
        };

        let curr_offset_pos = offsets_start + col_idx * 4;
        let cell_end = u32::from_le_bytes(
            self.payload[curr_offset_pos..curr_offset_pos + 4].try_into().unwrap()
        ) as usize;

        Some(&self.payload[data_start_base + cell_start..data_start_base + cell_end])
    }
}
```

#### B. Cơ chế `QueryResultPager` & Tràn đĩa `DiskSpiller`
* Mỗi tab kết quả giữ một `LruCache<u32, Arc<SlottedPage>>` giới hạn **1,024 trang (~64MB RAM)**.
* Khi trang thứ 1,025 được đẩy vào từ Database stream:
  1. Trang ít được truy cập nhất (LRU) được lấy ra, nén bằng `lz4_flex::compress_prepend_size()`.
  2. Ghi nối tiếp vào file tạm `/var/tmp/fastdb/query_{uuid}.spill` và lưu tọa độ `(file_offset: u64, compressed_len: u32)` vào bảng `SpillIndex`.
  3. Khi người dùng cuộn bảng về lại vùng trang đã đẩy xuống đĩa, `DiskSpiller` đọc qua `memmap2::Mmap` và giải nén LZ4 trả lại vào LRU Cache trong **< 0.4ms**.

---

### 4.3. Giao thức Database Nhị phân & Cầu nối Luồng (`crates/db-wire`)

* **Tách biệt hoàn toàn luồng Mạng và luồng UI:**
  * `Tokio Runtime` chạy ngầm với $N$ worker threads để xử lý kết nối TCP/Unix Socket, giải mã gói tin và nén LZ4.
  * `GPUI Main Thread` chỉ nhận tín hiệu thông báo nhẹ qua kênh `flume::Receiver<StreamEvent>`.

```rust
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use anyhow::Result;
use async_trait::async_trait;

pub enum StreamEvent {
    SchemaReady(Vec<ColumnMeta>),
    PageAppended { total_rows: u64 },
    Completed { total_rows: u64, elapsed_ms: u64 },
    Error(String),
}

#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// Thực thi query và đẩy thẳng luồng byte vào QueryResultPager
    async fn execute_streaming(
        &self,
        sql: &str,
        pager: Arc<QueryResultPager>,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()>;

    /// Hủy câu lệnh đang chạy ngay lập tức (Out-of-Band Cancel)
    async fn cancel_active_query(&self) -> Result<()>;
}
```

* **Cơ chế Throttled UI Notification (Chống nghẽn sự kiện vẽ):**
  * Khi tải 1 triệu dòng, `db-wire` **không** gọi `cx.notify()` ở từng dòng.
  * Worker chỉ gửi `StreamEvent::PageAppended` khi **đầy 1 trang 64KB** hoặc **sau mỗi 16ms** (tương ứng nhịp khung hình), giúp thanh cuộn và số đếm dòng tăng mượt mà không làm quá tải UI thread.

---

### 4.4. Bảng Dữ liệu Ảo hóa 120 FPS (`crates/ui-grid`)

* **Toán học Cắt Khung nhìn (2D Viewport Culling):**
  * Giả sử chiều cao mỗi dòng `ROW_HEIGHT = 26.0 px`, vị trí cuộn dọc hiện tại là `scroll_top`, chiều cao khung nhìn là `viewport_height`.
  * Chỉ số dòng bắt đầu và kết thúc cần vẽ trong khung hình hiện tại:
    * `first_row = (scroll_top / ROW_HEIGHT).floor() as usize`
    * `visible_count = (viewport_height / ROW_HEIGHT).ceil() as usize + 2`
    * `last_row = (first_row + visible_count).min(total_rows)`
  * Trong mỗi khung hình, bất kể kết quả có **100 dòng** hay **10,000,000 dòng**, vòng lặp `paint` của GPUI chỉ chạy đúng **~40 lần** (cho 40 dòng đang hiển thị trên màn hình).
* **Định dạng chuỗi Zero-Allocation trên Stack:**
  * Với các cột số nguyên (`INT4`, `INT8`) hoặc số thực (`FLOAT8`), dùng crate `itoa` và `ryu` để chuyển byte nhị phân sang chuỗi UTF-8 ngay trên mảng stack `[u8; 32]` rồi truyền thẳng vào hàm vẽ chữ của GPUI, **không cấp phát bất kỳ `String` nào trên Heap khi cuộn bảng**.

---

## 5. LỘ TRÌNH IMPLEMENT STEP-BY-STEP (CHECKLIST THỰC CHIẾN)

### Bước 1: Dựng khung Workspace & `platform-hal` (Ngày 1 – 3)
- [ ] Tạo cấu trúc thư mục theo mục 2 và file `Cargo.toml` gốc theo mục 3.
- [ ] Viết `crates/platform-hal/src/paths.rs`: Đảm bảo thư mục spill trên Linux trỏ về `/var/tmp/fastdb` và tự động dọn dẹp các file `.spill` cũ khi khởi động app.
- [ ] Viết `crates/platform-hal/src/socket.rs` và `keychain.rs`.
- [ ] Khởi tạo cửa sổ GPUI cơ bản trong `crates/app/src/main.rs`, nhúng font `JetBrains Mono` và `Inter`.
- [ ] **Kiểm thử:** Chạy `cargo run --release` trên cả macOS và Linux; xác nhận cửa sổ hiện lên dưới 100ms, RAM dưới 35MB.

### Bước 2: Implement `mem-engine` & Unit Test chịu tải (Ngày 4 – 8)
- [ ] Viết `SlottedPage` (64KB) với `Null Bitmap` và `Cell Offset` mảng 32-bit.
- [ ] Viết `DiskSpiller` sử dụng `lz4_flex` + `memmap2`.
- [ ] Viết `QueryResultPager` kết hợp `parking_lot::RwLock` và `LruCache` (1,024 trang).
- [ ] **Kiểm thử (`cargo test -p mem-engine`):** Viết bài test bơm 2,000,000 dòng × 15 cột vào Pager; kiểm tra RAM của test process không vượt quá 75MB và đọc ngẫu nhiên `get_cell()` chính xác 100%.

### Bước 3: Implement `db-wire` cho PostgreSQL (Ngày 9 – 14)
- [ ] Thiết lập kết nối `tokio-postgres` hỗ trợ tự động chuyển đổi giữa Unix Domain Socket và TCP.
- [ ] Viết `binary_decoder.rs`: Đọc luồng `RowStream` nhị phân của Postgres (`INT2`, `INT4`, `INT8`, `FLOAT4`, `FLOAT8`, `NUMERIC`, `TEXT`, `VARCHAR`, `TIMESTAMPTZ`, `UUID`, `JSONB`) và ghi trực tiếp vào `SlottedPage`.
- [ ] Cài đặt `cancel_active_query()` qua `CancelToken` của Postgres.
- [ ] **Kiểm thử:** Kết nối vào Postgres thật, chạy query lấy 500,000 dòng và đo thời gian nhận trang đầu tiên (< 10ms).

### Bước 4: Xây dựng `ui-grid` trên GPUI (Ngày 15 – 22)
- [ ] Viết custom element `VirtualDataGrid` trong `crates/ui-grid/src/grid_element.rs` thực hiện 2D Viewport Culling cho cả dòng và cột.
- [ ] Cài đặt tính năng kéo giãn độ rộng cột (Column Resize), cố định tiêu đề cột (Sticky Header) và số thứ tự dòng.
- [ ] Viết `selection.rs`: Hỗ trợ click chọn ô, kéo chuột chọn vùng, bấm `Cmd/Ctrl + C` để copy ra TSV/CSV/SQL Insert.
- [ ] Viết `changeset.rs`: Nhấp đúp vào ô để sửa trực tiếp, đổi màu các ô đã sửa, xem trước câu lệnh `UPDATE` và bấm `Cmd/Ctrl + S` để thực thi.

### Bước 5: Xây dựng `ui-editor` & `sql-intel` (Ngày 23 – 30)
- [ ] Viết `SqlEditorView` dựa trên `ropey::Rope` hỗ trợ cuộn mượt với file SQL lớn.
- [ ] Tích hợp `tree-sitter-sequel` để tô màu từ khóa SQL và tự động đóng khung câu lệnh tại vị trí con trỏ chuột.
- [ ] Viết `schema_cache.rs`: Truy vấn danh sách schema/table/column từ Postgres nạp vào SQLite In-Memory cục bộ ngay khi kết nối thành công.
- [ ] Hiển thị popup Autocomplete khi người dùng gõ tên bảng, alias hoặc dấu chấm (`.`).

### Bước 6: Tích hợp SSH Tunnel, MySQL/SQLite & Đóng gói (Ngày 31 – 36)
- [ ] Tích hợp `russh` để hỗ trợ kết nối Database qua SSH Jump Host (hỗ trợ khóa `ed25519`, `rsa` và `SSH Agent` của macOS/Linux).
- [ ] Bổ sung bộ đọc binary stream cho MySQL (`mysql_async`) và SQLite (`rusqlite`).
- [ ] Thêm tính năng **Streaming Export**: Xuất thẳng từ `db-wire` ra file `.csv` hoặc `.sql` mà không nạp vào giao diện.
- [ ] Đóng gói bản phân phối:
  * **macOS:** Tạo ứng dụng `.app` và nén `.dmg` cho Apple Silicon (`aarch64-apple-darwin`).
  * **Linux:** Đóng gói `.deb` và `.AppImage` trên môi trường Ubuntu 24.04 LTS.
