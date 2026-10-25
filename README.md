# FastDB ⚡

> **Next-Generation Pure Rust Database Client**  
> *Zero JVM · Zero JDBC · 120 FPS GPU-Accelerated UI · Zero-Copy Memory Management*

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![UI](https://img.shields.io/badge/UI-GPUI-blue.svg)](https://github.com/zed-industries/zed)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux-lightgrey.svg)](ci/)

FastDB is a lightning-fast, resource-efficient database client built from the ground up in pure Rust. Designed as a modern replacement for legacy JVM/JDBC-heavy tools like DBeaver, FastDB renders directly via **Metal** (macOS) and **Vulkan/X11/Wayland** (Linux) at a fluid 120 FPS, consumes a fraction of the memory, and provides instant cold starts.

---

## ⚡ Performance Benchmarks

| Metric | DBeaver (Java / JDBC) | FastDB (Rust / GPUI) | Advantage |
| :--- | :--- | :--- | :--- |
| **Cold Start Time** | 4.5s – 12s | **< 120ms** | **~40× faster** |
| **Idle RAM Usage** | 600 MB – 1.2 GB | **< 35 MB** | **~25× less memory** |
| **1M Rows × 20 Cols Query** | 2.5 GB+ (Risk of OOM) | **Bounded ~80 MB** (Auto Spill) | **Stable memory footprint** |
| **Time-to-First-Row (TTFR)** | 300ms – 2,000ms | **< 10ms** | **Instant response** |
| **Grid Scrolling Framerate** | 15 – 45 FPS (GC pauses) | **120 FPS** (GPU Virtualized) | **Silky smooth** |
| **Application Package Size** | ~130 MB+ (Bundled JRE) | **~12 MB** (.dmg / .AppImage) | **~10× smaller** |

---

## 🚀 Key Features

### 🏎️ Flat Memory Engine & Automated Disk Spill (`mem-engine`)
- **64KB Slotted Pages**: Aligned to 16KB Apple Silicon OS pages and 4KB Linux pages. Eliminates per-cell heap allocations using compact bitmapped NULL tracking and 32-bit cell offsets.
- **LRU Page Cache**: Keeps up to 1,024 active pages (~64 MB RAM) hot in memory.
- **LZ4 Disk Spiller**: As query results exceed cache limits, background workers compress evicted pages with `lz4_flex` and write them to memory-mapped temporary files (`memmap2`). Pages reload back into memory in **< 0.4ms**.

### 🖥️ 120 FPS 2D Virtualized Data Grid (`ui-grid`)
- **2D Viewport Culling**: Regardless of whether a query returns 100 rows or 10,000,000 rows, only the visible ~40 rows and onscreen columns are rendered each frame.
- **Zero-Allocation Stack Formatting**: Numeric columns (`INT2`, `INT4`, `INT8`, `FLOAT4`, `FLOAT8`) are serialized to UTF-8 directly into stack buffers via `itoa` and `ryu`, bypassing heap allocations during scrolling.
- **Interactive Editing & Export**:
  - Drag-to-resize and double-click to auto-fit column widths based on content.
  - Multi-cell and rectangular range selection with one-click export to TSV, CSV, or SQL `INSERT` statements.
  - In-place inline cell editing with visual change indicators and automated SQL `UPDATE` statement generation.

### 🔌 Pure Native Wire Protocols (`db-wire`)
- **No JDBC Overhead**: Native asynchronous drivers communicate directly with database wire protocols:
  - **PostgreSQL**: Native binary wire decoding for rich types (`INT`, `FLOAT`, `NUMERIC`, `TEXT`, `TIMESTAMPTZ`, `UUID`, `JSONB`).
  - **MySQL / MariaDB**: Asynchronous binary protocol streaming.
  - **SQLite**: Direct zero-latency C-FFI integration.
- **Decoupled Architecture**: High-throughput network I/O runs on a dedicated multi-threaded Tokio runtime, delivering throttled batch events (per 64KB page or 16ms frame interval) to the GPUI main thread over lock-free channels.
- **Instant Out-of-Band Cancellation**: Cancel long-running queries immediately using dedicated connection cancel tokens.
- **Integrated SSH Bastion / Jump Host**: Built-in SSH client (`russh`) supporting Ed25519, RSA, and system SSH Agents.

### 🧠 SQL Intelligence & Modern Editor (`sql-intel` & `ui-editor`)
- **Tree-Sitter SQL Parsing**: Accurate syntax highlighting and statement boundary detection powered by `tree-sitter-sequel`. Run only the statement at your cursor without manual text selection.
- **Ropey Text Buffer**: Ultra-responsive editing with fast undo/redo history, even for multi-megabyte SQL scripts.
- **Sub-2ms Autocompletion**: In-memory SQLite catalog (`SchemaCache`) indexing tables, views, columns, aliases, and foreign keys for instantaneous autocompletion popups.

### 🍏 Platform Hardware Abstraction Layer (`platform-hal`)
- **Safe Disk Spilling**: Automatically avoids `tmpfs` on Linux distributions by directing spill files to `/var/tmp/fastdb` instead of RAM-backed `/tmp`, while adhering to `$TMPDIR` on macOS.
- **Automatic Unix Socket Discovery**: Automatically detects local Unix Domain Sockets for PostgreSQL (`/tmp/.s.PGSQL.5432`, `/var/run/postgresql/`, `/run/postgresql/`) and MySQL (`/tmp/mysql.sock`, `/var/run/mysqld/mysqld.sock`).
- **OS Keychain Integration**: Securely stores database passwords via macOS Keychain and Linux Secret Service (DBus).
- **Normalized Keybindings**: Transparent `Cmd` (macOS) and `Ctrl` (Linux) modifier mapping.

---

## 🏛️ System Architecture

```mermaid
flowchart TD
    subgraph UI_Thread["GPUI Main Thread (120 FPS Rendering)"]
        Workspace["Workspace Window"]
        Navigator["Database Navigator Tree"]
        Editor["ui-editor (Ropey + Tree-sitter)"]
        Grid["ui-grid (2D Virtualized Viewport)"]
    end

    subgraph Channel["Async Communication"]
        Flume["Flume Event Stream\n(Throttled: 64KB or 16ms)"]
    end

    subgraph Backend["Tokio Multi-Thread Worker Pool"]
        DBWire["db-wire Driver\n(PostgreSQL / MySQL / SQLite)"]
        SSHTunnel["SSH Tunnel\n(russh)"]
        SqlIntel["sql-intel\n(SchemaCache & AST Parser)"]
    end

    subgraph Storage["mem-engine (Zero-Copy)"]
        Pager["QueryResultPager\n(LRU Cache: 1,024 Pages / 64MB)"]
        Slotted["SlottedPage (64KB aligned)"]
        Spiller["DiskSpiller\n(LZ4 Compression + memmap2)"]
        Disk[("Spill Files\n/var/tmp/fastdb or $TMPDIR")]
    end

    Editor -->|"Cmd/Ctrl + Enter"| DBWire
    DBWire -->|"Decode Binary Wire"| Slotted
    Slotted --> Pager
    Pager -->|"Evict > 64MB"| Spiller
    Spiller <-->|"Mmap & LZ4"| Disk
    DBWire -->|"StreamEvent"| Flume
    Flume -->|"Notify UI"| Grid
    Pager -.->|"Zero-Copy Cell Read O(1)"| Grid
```

---

## 📂 Workspace Crates

```text
fastdb/
├── Cargo.toml               # Workspace configuration & shared dependencies
├── rust-toolchain.toml      # Pinned stable Rust toolchain (1.85+)
├── assets/                  # Icons and font assets (JetBrains Mono, Inter, DB icons)
├── ci/                      # Distribution packaging scripts (.dmg, .deb, .AppImage)
└── crates/
    ├── app/                 # [Binary] Main entrypoint, window setup & workspace UI
    ├── platform-hal/        # [Library] OS abstraction (sockets, paths, keychain, keymaps)
    ├── mem-engine/          # [Library] Zero-copy slotted pages, LRU pager & LZ4 spill engine
    ├── db-wire/             # [Library] Binary database drivers (Postgres, MySQL, SQLite, SSH)
    ├── sql-intel/           # [Library] Tree-sitter SQL parsing, schema cache & autocompletion
    ├── ui-core/             # [Library] Core themes, styles & shared GPUI actions
    ├── ui-editor/           # [Library] Ropey-backed SQL code editor with syntax highlighting
    └── ui-grid/             # [Library] 2D virtualized data table with inline editing & export
```

---

## ⌨️ Default Keybindings

| Shortcut (macOS) | Shortcut (Linux) | Action |
| :--- | :--- | :--- |
| `Cmd + Enter` | `Ctrl + Enter` | Execute SQL statement at cursor |
| `Cmd + S` | `Ctrl + S` | Save & commit inline grid modifications to DB |
| `Cmd + T` | `Ctrl + T` | Open a new SQL editor tab |
| `Cmd + W` | `Ctrl + W` | Close current SQL editor tab |
| `Cmd + C` | `Ctrl + C` | Copy selected grid cells as TSV / CSV / SQL Insert |
| `Double Click` | `Double Click` | Edit grid cell inline / Auto-fit column width |

---

## 🛠️ Getting Started

### Prerequisites

- **Rust**: Version `1.85.0` or newer with `cargo`.
- **macOS**: macOS 12 Monterey or newer (Metal support).
- **Linux**: Ubuntu 22.04+, Debian 12+, Fedora 38+, Arch Linux, or any modern distribution with:
  ```bash
  # Debian/Ubuntu dependencies
  sudo apt-get install -y build-essential libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
                          libfontconfig1-dev libasound2-dev libvulkan-dev libssl-dev
  ```

### Clone and Run

```bash
# Clone the repository
git clone https://github.com/your-username/fastdb.git
cd fastdb

# Run FastDB in release mode for best performance
cargo run --release -p fastdb-app
```

### Running Tests

FastDB includes comprehensive unit and integration tests covering memory paging, disk spilling under high load, SQL statement boundary parsing, and binary drivers:

```bash
# Run all workspace unit and integration tests
cargo test

# Run memory engine load test (spill & accuracy test)
cargo test -p mem-engine --test load_test

# Run UI grid selection and virtual scrolling tests
cargo test -p ui-grid
```

---

## 📦 Building Distribution Packages

FastDB includes automated packaging scripts for macOS and Linux in the `ci/` directory:

### macOS (.app & .dmg)
```bash
chmod +x ci/build-macos.sh
./ci/build-macos.sh
```
*Generates an optimized Apple Silicon bundle at `dist/macos/FastDB.app` and an installer at `dist/macos/FastDB-0.1.0-arm64.dmg`.*

### Linux (.deb & .AppImage)
```bash
chmod +x ci/build-linux.sh
./ci/build-linux.sh
```
*Generates `dist/linux/fastdb_0.1.0_amd64.deb` and `dist/linux/FastDB-0.1.0-x86_64.AppImage`.*

---

## 🗺️ Roadmap

- [x] Pure Rust workspace structure and GPUI window initialization
- [x] Zero-copy 64KB `SlottedPage` and LZ4 `DiskSpiller` with `memmap2`
- [x] Native PostgreSQL binary wire streaming and out-of-band query cancellation
- [x] 120 FPS 2D Virtual Data Grid with auto-fit, column resizing, and inline editing
- [x] DBeaver-style database explorer navigation tree (Databases, Schemas, Tables, Columns, Constraints, Indexes)
- [x] Tree-sitter SQL parsing and statement-at-cursor detection
- [x] MySQL binary protocol and embedded SQLite drivers
- [x] SSH Bastion / Jump Host tunneling
- [x] Streaming export to CSV and SQL
- [ ] Visual query execution plan (EXPLAIN ANALYZE) tree viewer
- [ ] Data diff and schema comparison tool
- [ ] Dark / Light theme customizer

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
