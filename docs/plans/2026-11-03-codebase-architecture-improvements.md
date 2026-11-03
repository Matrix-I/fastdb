# Codebase Architecture, God Files, and Technical Debt Improvement Plan

Date: 2026-11-03  
Status: Documented / Backlog for Incremental Fixes

## 1. Goal

Address critical performance bottlenecks, architectural smells, and "god file" anti-patterns identified during the deep architectural audit. Preserve FastDB's lightweight native GPUI architecture while decoupling monolithic state, eliminating massive GUI element churn, securing data mutations, and removing redundant heap allocations.

---

## 2. Codebase Baseline & Structural Audit

### 2.1 Workspace Overview

As of 2026-11-03, the FastDB repository contains **152 Rust source files** and **40,569 lines of code** across 8 crates and the `xtask` benchmark automation runner:

| Crate / Directory | Files | LOC | Primary Responsibility |
| :--- | :---: | :---: | :--- |
| `crates/app` | 78 | 23,481 | Main executable, GPUI composition root, layout, dialogs, updater (**57.9%**) |
| `crates/db-wire` | 32 | 8,005 | Database driver abstraction (Postgres, MySQL, SQLite, ODBC), streaming sink |
| `crates/ui-editor` | 7 | 2,897 | SQL text editor (Ropey buffer, view, single-line input, autocomplete popup) |
| `crates/ui-grid` | 10 | 2,192 | Virtualized tabular result grid, column sizing, selection, changeset editing |
| `crates/sql-intel` | 6 | 1,428 | Tree-sitter SQL parsing, statement splitter, SQLite in-memory schema cache |
| `xtask` | 4 | 981 | Automation harness measuring startup, RSS, query throughput, scroll latency |
| `crates/mem-engine` | 6 | 888 | Slotted page storage (64KB), LRU buffer cache, LZ4 disk spilling |
| `crates/platform-hal` | 5 | 421 | OS paths, OS Keychain store (`keyring`), Unix socket auto-discovery |
| `crates/ui-core` | 4 | 276 | Shared actions, undo/redo history stack, theme tokens |
| **Total** | **152** | **40,569** | |

---

## 3. God Files and Structural Anti-Patterns

### 3.1 Struct `Workspace` (`crates/app/src/workspace/mod.rs` & 40 partial impl files)
- **Scale**: Struct `Workspace` contains **33 direct fields** and is implemented across **40 separate files** totaling **14,250 LOC**.
- **Smell**: Acts as a "Partial Mega-Class" where all 40 files share unfettered mutable access to `self.tabs`, `self.active_tab`, `self.form`, `self.connections`, and `self.operations`.
- **Issues**:
  - Lack of encapsulation: any component can mutate unrelated state without boundaries.
  - Blocking synchronous disk I/O occurs on the main UI thread during `Workspace::new` (`ConnectionRepository::load_saved_connections()` reads JSON before rendering frame 1).

### 3.2 Struct `TabState` (`crates/app/src/workspace/tab_state.rs`)
- **Scale**: Struct `TabState` holds **45 fields** in 1,072 LOC.
- **Smell**: Monolithic untyped tab representation combining two completely disjoint screens:
  - *Table View*: `schema_name`, `table_name`, `filter_sql`, `filter_timeline`, `active_subtab`, `active_property_section`, `diagram`.
  - *Script View*: `editor` (`SqlEditorView`), `script_path`, `saved_text`, `results`, `tab_session`, `pending_statements`.
- **Issues**:
  - Memory bloat: opening a Table Tab instantiates a full `SqlEditorView` with its Ropey buffer, tree-sitter parser, and autocomplete state.
  - Fragile heuristic identification: `is_script()` checks `self.column_editor.is_none() && self.table_name.is_none()`.

### 3.3 File `updater.rs` (`crates/app/src/updater.rs`)
- **Scale**: 1,199 LOC single file.
- **Smell**: Violates Single Responsibility Principle by combining GitHub REST API calls, semver parsing, file download streaming, deep OS shell automation (`hdiutil attach/detach`, `ditto`, `codesign`), and process respawning.
- **Security Issue**: Ad-hoc re-signs binaries (`codesign --force --deep --sign -`) without cryptographic signature verification (Minisign) or SHA256 checksum validation.

---

## 4. Bad Designs & Performance Bottlenecks

### 4.1 SQL Editor GPUI Element Explosion ("One-Div-Per-Character")
- **Location**: `crates/ui-editor/src/editor_view.rs:621-645`
- **Issue**: Instead of using GPUI's text shaping (`TextSystem`, `ShapedRun`) to render lines in single draw passes, the editor generates **3 `div` elements for every character** (one container + two child hit-test divs for cursor positioning) plus a heap-allocated `String`.
- **Impact**: A 50-line view (80 chars/line) creates **~12,000 GPUI elements** per render pass, causing noticeable typing and cursor latency.

### 4.2 Non-Functional SSH Tunneling (Dead Code Stub)
- **Location**: `crates/db-wire/src/ssh_tunnel.rs:53-70`
- **Issue**: `SshTunnel::start_forwarding` binds a local TCP port, prints a log pretending the tunnel is established, and immediately drops the `TcpListener` on return. No socket forwarding loop exists, and the function is not called anywhere in the workspace.

### 4.3 7 Heap Allocations Per Streamed Row
- **Location**: `crates/db-wire/src/stream.rs:62`, `crates/mem-engine/src/pager.rs:72, 76`, `crates/mem-engine/src/slotted_page.rs:95, 117, 128, 129`
- **Issue**: On every single streamed row, 7 temporary vectors are allocated on the heap: `cell_slices`, `cells_vec`, `row_slices`, `cell_vec`, `null_bitmap`, `cell_offsets`, and `data_bytes`.
- **Impact**: Streaming 200,000 rows generates **1,400,000 short-lived heap vectors**, resulting in severe allocator churn.

### 4.4 Large Rows Bypassing SlottedPage 64KB Assumption
- **Location**: `crates/mem-engine/src/slotted_page.rs:109` & `pager.rs:96-100`
- **Issue**: `try_append_row` only checks `total_est > PAGE_SIZE` if `self.row_count > 0`. On a fresh page (`row_count == 0`), oversized rows (>64KB) are written directly.
- **Impact**: While preventing query abortion, it breaks `QueryResultPager`'s assumption that 128 pages $\approx$ 8MB RAM. A few large JSON/BLOB rows can inflate memory to gigabytes without triggering LRU disk eviction.

### 4.5 Unparameterized SQL String Formatting in Grid Edits
- **Location**: `crates/db-wire/src/sql.rs:30-50`
- **Issue**: Cell updates during inline grid editing are formatted into raw SQL via string interpolation and single-quote escaping (`v.replace('\'', "''")`) rather than parameterized queries (`$1`, `?`).
- **Impact**: Security risk and syntax failure on complex data types (binary BLOBs with `\0`, complex JSON, timestamps with timezone).

### 4.6 Hardcoded Character Width Constants in Column Sizing
- **Location**: `crates/ui-grid/src/grid_element/widths.rs:10-25`
- **Issue**: Column auto-sizing estimates character widths via hardcoded ASCII float tables (`3.8`, `5.2`, `9.5`), dumping all non-ASCII characters into a fallback `6.5` constant.
- **Impact**: Column widths are incorrectly calculated for Vietnamese, CJK, and other Unicode text, causing truncated cell values.

### 4.7 Divergent Tokenizers in `sql-intel`
- **Location**: `crates/sql-intel/src/parser.rs` vs `crates/sql-intel/src/script.rs`
- **Issue**: Two distinct parsers coexist: Tree-sitter (`tree-sitter-sequel`) for syntax highlighting and statement detection, and a 550-line custom hand-written byte lexer in `script.rs`. The custom lexer rejects MySQL comment syntax (`#` or `/*!... */`) that Tree-sitter accepts, creating semantic divergence between editing and execution.

### 4.8 Lock Contention in ODBC Query Cancellation
- **Location**: `crates/db-wire/src/odbc.rs:130-137, 411-436`
- **Issue**: Statement raw handles are shared via `Arc<Mutex<Option<usize>>>`. If `SQLCancel` blocks or the driver's thread-safety is misconfigured on Linux/macOS `unixODBC`, `ActiveStatement::drop` can deadlock Tokio worker threads.

### 4.9 Unnecessary Heap Copy in Grid Render Loop
- **Location**: `crates/ui-grid/src/grid_element.rs:89-92`
- **Issue**: For every visible cell on screen, `pager.get_cell` allocates a new `Vec<u8>` followed by `String::from_utf8_lossy(...).into_owned()`, ignoring the zero-copy closure API `pager.with_cell(...)`.

---

## 5. Phased Implementation Roadmap

### Phase 1: Critical UI & Concurrency Fixes (P1)

- [ ] **P1-01 — Replace One-Div-Per-Character in SQL Editor.**
  - Migrate `crates/ui-editor/src/editor_view.rs` to use GPUI's `TextSystem` / `ShapedRun` text shaping API for entire lines.
  - Calculate cursor placement and selection boundaries using line x-coordinates instead of per-character hit-test divs.
  - *Acceptance*: Typing latency dropped, smooth 60fps scrolling, GPUI element count reduced by >90%.

- [ ] **P1-02 — Multi-tab Query Concurrency.**
  - Refactor `Operations` in `crates/app/src/workspace/operations.rs` from `active: Option<Operation>` to a keyed registry `HashMap<TabId, Operation>`.
  - Ensure background streaming tasks report progress and completion to their owning tab ID.
  - *Acceptance*: Users can execute queries simultaneously across different tabs/databases without blocking.

- [ ] **P1-03 — Type-Safe `TabKind` Enum Refactoring.**
  - Split `TabState` into `enum TabKind { Table(TableTabState), Script(ScriptTabState) }`.
  - Remove `SqlEditorView` allocation from Table tabs.
  - Replace heuristic `is_script()` with explicit pattern matching.
  - *Acceptance*: Reduced memory consumption when opening multiple table tabs; no dead fields.

---

### Phase 2: Data Pipeline, Security & Performance Fixes (P2)

- [ ] **P2-01 — Zero-Allocation Streaming Pipeline.**
  - Refactor row intake across `stream.rs`, `pager.rs`, and `slotted_page.rs`.
  - Use a reusable row buffer / slice borrowing (`&[&[u8]]`) to serialize directly into `SlottedPage.payload` without intermediate `Vec` allocations.
  - *Acceptance*: Memory allocator churn during large queries reduced by >80%; query streaming throughput improved.

- [ ] **P2-02 — Parameterized Queries for Data Editing.**
  - Update `crates/db-wire/src/sql.rs` and `crates/app/src/workspace/data_edit.rs` to construct parameterized queries with driver-level bind parameters (`$1`, `?`).
  - *Acceptance*: Reliable saving of BLOBs, JSON, and special characters; elimination of SQL injection vulnerabilities.

- [ ] **P2-03 — Zero-Copy Grid Rendering and Export.**
  - Refactor `crates/ui-grid/src/grid_element.rs` and `crates/app/src/workspace/export.rs` to use `QueryResultPager::with_cell`.
  - Borrow byte slices directly for text clipping and CSV/TSV serialization.
  - *Acceptance*: No micro-stutters during rapid grid scrolling; export performance improved.

- [ ] **P2-04 — Resolve SSH Tunneling Stub.**
  - Either implement a true TCP forwarding loop using `russh` channels in `crates/db-wire/src/ssh_tunnel.rs`, or explicitly disable/hide SSH configuration in `ConnectionDialog` until implemented.
  - *Acceptance*: No silent connection drops or misleading UI promises.

- [ ] **P2-05 — SlottedPage Oversized Row & Memory Accounting.**
  - Add explicit byte-size tracking to `QueryResultPager` so LRU eviction operates on total resident bytes rather than a naive 64KB $\times$ page count assumption.
  - Implement dedicated overflow/spill pages for rows exceeding 64KB.
  - *Acceptance*: Large JSON/LOB queries do not cause unbounded RAM growth.

---

### Phase 3: Architecture Consolidation (P3)

- [ ] **P3-01 — Unify SQL Parsing under Tree-sitter.**
  - Consolidate statement splitting and formatting in `crates/sql-intel` onto the Tree-sitter AST parser.
  - Remove the 550-line custom hand-written lexer in `script.rs`.
  - *Acceptance*: Consistent syntax support (including MySQL comments) across editor highlighting and execution.

- [ ] **P3-02 — Lock-Free Trie Schema Cache for Autocomplete.**
  - Replace the SQLite in-memory database in `crates/sql-intel/src/schema_cache.rs` with an in-memory prefix trie (e.g., `radix_trie` or `fst`).
  - *Acceptance*: Autocomplete suggestions return in <1ms without Mutex locking or SQL execution overhead.

- [ ] **P3-03 — Standardize Keyboard Shortcuts via GPUI Action Context.**
  - Refactor `crates/app/src/workspace/keyboard.rs` (466 LOC key interceptor) into GPUI `Action` definitions registered on specific focus contexts.
  - *Acceptance*: Clean action routing; enables user-configurable keybindings.

- [ ] **P3-04 — Modularize `Workspace` Partial Implementations.**
  - Decompose `Workspace` into discrete sub-components (`ConnectionManager`, `TabManager`, `FilterBuilderModal`, `ColumnEditorModal`) owning their respective state.
  - Remove partial `impl Workspace` blocks across the 40 files in `crates/app/src/workspace/`.
  - *Acceptance*: Clear state boundaries; unit-testable components.

- [ ] **P3-05 — Cryptographic Verification for Auto-Updater.**
  - Integrate Minisign public key verification (`resources/minisign.pub`) and SHA256 checksum checks into `crates/app/src/updater.rs` before invoking macOS packaging scripts.
  - *Acceptance*: Downloaded artifacts are cryptographically verified before installation.
