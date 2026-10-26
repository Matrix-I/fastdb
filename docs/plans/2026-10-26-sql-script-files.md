# SQL Script Files Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make SQL scripts DBeaver-like: every script is a `.sql` file that belongs to one connection and is saved automatically; "+ SQL" / Cmd+] reopens the connection's most recent script instead of always creating a new one, and when that script is already open it shows a "Choose SQL script" dialog (search, New script, show scripts for all connections, list by time).

**Architecture:** A pure, filesystem-backed `ScriptStore` (new `script_store.rs`) lists, creates, reads and writes `<config_dir>/scripts/<connection id>/Script-<n>.sql`, numbering new scripts after the highest existing number so names never pile up from in-memory counters. Script tabs remember their file and connection; text is written back when it changes (on the existing 530 ms caret timer), when a statement runs, when the tab closes and before quitting. A new `ScriptChooser` overlay (new `script_chooser.rs`) renders the dialog. Activating a script tab whose connection is not the active one switches to it, so a script always runs on its own connection.

**Tech Stack:** Rust, GPUI 0.2.2, `std::fs`, `platform-hal::paths::config_dir`, `chrono` (already an app dependency) for the time column.

## Global Constraints

- Code, comments, identifiers, commit messages and docs in English; match the surrounding style.
- In `#[cfg(test)]` modules of files that glob-import `super::*` from GPUI code, import names explicitly (`gpui::test` shadows `#[test]`).
- Tests that touch the filesystem use a fresh directory under `std::env::temp_dir()` and remove it at the end; never the real config directory.
- Script files are UTF-8 `.sql`; names are `Script-<n>.sql` (`n` ≥ 1).
- No new crate dependencies.
- Work lands on `main` (feature branch merged when done), no attribution lines in commits.

## Behaviour (spec)

1. **+ SQL / Cmd+] / navigator "SQL Editor"** act on a connection: the clicked one for the navigator item, otherwise the active connection. With no connection, the connection dialog opens (as running a query does today).
   - The connection has no scripts → create `Script-<n>.sql`, open it.
   - Its most recent script (by modification time) is not open in a tab → open it.
   - Its most recent script is already open → show the chooser dialog for that connection.
2. **Chooser dialog** "Choose SQL script for '<connection name>'": a search box (filters by file name and first line, case-insensitive), a **New script** button, a **Show scripts for all connections** checkbox, and a list with columns *Script*, *Time* (`YYYY-MM-DD HH:MM:SS`, newest first), *Info* (first non-empty line, trimmed) and — when all connections are shown — *Connection*. ↑/↓ move the selection, Enter or double-click opens it, Esc or a click outside closes the dialog. Opening a script that is already open activates its tab.
3. **Saving:** a script tab's text is written to its file whenever it differs from what was last written: on every caret-timer tick (530 ms), before running a statement, when the tab closes, and before Cmd+Q quits. Write errors are shown in the tab's error banner.
4. **Connection binding:** a script tab shows its connection's name in its toolbar. Selecting the tab (click, chooser, Cmd+]) switches the active connection to the script's connection if needed (`connect_saved_by_id`).
5. Tab titles are the file stem (`Script-38`).

## Out of scope

Renaming/deleting scripts, folders, the Info column for multi-line previews, choosing a different connection for an existing script, scripts for table tabs.

---

## File Structure

| File | Change | Responsibility |
| :--- | :--- | :--- |
| `crates/app/src/workspace/script_store.rs` | Create | `ScriptStore`, `ScriptFile`: list / most recent / create / read / write script files; `filter_scripts` for the dialog search |
| `crates/app/src/workspace/script_chooser.rs` | Create | `ScriptChooser` state, its key handling and rendering |
| `crates/app/src/workspace/query.rs` | Modify | `TabState::script` takes a `ScriptFile` and its text; fields `script_path`, `connection_id`, `saved_text`; `open_sql_script(conn_id)` implementing the spec; `save_script_tab` / `save_dirty_scripts` |
| `crates/app/src/workspace/sql_script.rs` | Modify | Remove `script_title` (names now come from files) |
| `crates/app/src/workspace/mod.rs` | Modify | Modules, `script_store: ScriptStore` and `script_chooser: Option<ScriptChooser>` fields, drop `next_script_number`, autosave in the caret timer, render the chooser |
| `crates/app/src/workspace/keyboard.rs` | Modify | Route keys to the chooser while open; save before Cmd+Q |
| `crates/app/src/workspace/top_bar.rs`, `tab_context_menu.rs` | Modify | Switch connection when a script tab is selected; save before closing tabs |
| `crates/app/src/workspace/nav_context_menu.rs` | Modify | "SQL Editor" calls `open_sql_script(Some(conn_id))` |
| `crates/app/src/workspace/script_view.rs` | Modify | Connection name in the toolbar |
| `crates/app/src/assets.rs` | Modify | Tests use the new `TabState::script` signature |
| `README.md` | Modify | Describe saved scripts and the chooser |

---

### Task 1: `ScriptStore`

**Files:**
- Create: `crates/app/src/workspace/script_store.rs`
- Modify: `crates/app/src/workspace/mod.rs` (`mod script_store;`, `pub use script_store::{ScriptFile, ScriptStore};`)

**Interfaces (produces):**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFile {
    pub path: PathBuf,
    pub connection_id: String,
    /// File stem, shown as the tab title ("Script-3")
    pub name: String,
    pub modified: SystemTime,
    /// First non-empty line, trimmed, for the chooser's Info column
    pub preview: String,
}

pub struct ScriptStore { root: PathBuf }

impl ScriptStore {
    pub fn new(root: PathBuf) -> Self;
    /// `<config_dir>/scripts`
    pub fn default_root() -> PathBuf;
    /// Scripts of one connection, or of all connections for `None`, newest first
    pub fn list(&self, connection_id: Option<&str>) -> Vec<ScriptFile>;
    pub fn most_recent(&self, connection_id: &str) -> Option<ScriptFile>;
    /// Creates an empty `Script-<max+1>.sql` for the connection
    pub fn create(&self, connection_id: &str) -> anyhow::Result<ScriptFile>;
    pub fn read(&self, script: &ScriptFile) -> anyhow::Result<String>;
    pub fn write(&self, path: &Path, text: &str) -> anyhow::Result<()>;
}

/// Scripts whose name or preview contains `query` (case-insensitive); all for an empty query
pub fn filter_scripts<'a>(scripts: &'a [ScriptFile], query: &str) -> Vec<&'a ScriptFile>;
```

- [ ] **Step 1: Write the failing tests** (bottom of `script_store.rs`, with stub bodies `todo!()` above)

```rust
#[cfg(test)]
mod tests {
    use super::{filter_scripts, ScriptStore};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fastdb-scripts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn touch(path: &std::path::Path, secs_ago: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(secs_ago)).unwrap();
    }

    #[test]
    fn new_scripts_continue_after_the_highest_number() {
        let root = temp_root("numbering");
        let store = ScriptStore::new(root.clone());
        assert_eq!(store.create("pg").unwrap().name, "Script-1");
        assert_eq!(store.create("pg").unwrap().name, "Script-2");
        std::fs::write(root.join("pg").join("Script-38.sql"), "").unwrap();
        assert_eq!(store.create("pg").unwrap().name, "Script-39");
        // Numbering is per connection
        assert_eq!(store.create("mysql").unwrap().name, "Script-1");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_is_newest_first_with_a_preview_and_per_connection() {
        let root = temp_root("list");
        let store = ScriptStore::new(root.clone());
        let old = store.create("pg").unwrap();
        store.write(&old.path, "\n  SELECT 1;\nSELECT 2;").unwrap();
        touch(&old.path, 60);
        let new = store.create("pg").unwrap();
        store.write(&new.path, "drop index x;").unwrap();
        store.create("mysql").unwrap();

        let pg = store.list(Some("pg"));
        assert_eq!(pg.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Script-2", "Script-1"]);
        assert_eq!(pg[1].preview, "SELECT 1;");
        assert_eq!(pg[0].connection_id, "pg");
        assert_eq!(store.most_recent("pg").unwrap().name, "Script-2");
        assert_eq!(store.list(None).len(), 3);
        assert_eq!(store.read(&pg[1]).unwrap(), "\n  SELECT 1;\nSELECT 2;");
        assert!(store.most_recent("sqlite").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_script_files_are_ignored() {
        let root = temp_root("ignore");
        let store = ScriptStore::new(root.clone());
        store.create("pg").unwrap();
        std::fs::write(root.join("pg").join("notes.txt"), "x").unwrap();
        assert_eq!(store.list(Some("pg")).len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn filter_matches_name_or_preview_ignoring_case() {
        let root = temp_root("filter");
        let store = ScriptStore::new(root.clone());
        let a = store.create("pg").unwrap();
        store.write(&a.path, "explain analyze select 1").unwrap();
        store.create("pg").unwrap();
        let all = store.list(Some("pg"));
        assert_eq!(filter_scripts(&all, "").len(), 2);
        assert_eq!(filter_scripts(&all, "EXPLAIN")[0].name, "Script-1");
        assert_eq!(filter_scripts(&all, "script-2")[0].name, "Script-2");
        assert!(filter_scripts(&all, "nothing").is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `SDKROOT=$(xcrun --show-sdk-path) cargo test -p fastdb-app script_store`
Expected: FAIL (`not yet implemented`).

- [ ] **Step 3: Implement**

```rust
//! SQL scripts saved as `.sql` files, one folder per connection:
//! `<config_dir>/scripts/<connection id>/Script-<n>.sql`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

const PREFIX: &str = "Script-";

impl ScriptStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_root() -> PathBuf {
        platform_hal::paths::config_dir().join("scripts")
    }

    fn dir(&self, connection_id: &str) -> PathBuf {
        self.root.join(connection_id)
    }

    /// Number of a `Script-<n>.sql` file name
    fn number(file_name: &str) -> Option<u64> {
        file_name.strip_prefix(PREFIX)?.strip_suffix(".sql")?.parse().ok()
    }

    fn list_dir(&self, connection_id: &str, out: &mut Vec<ScriptFile>) {
        let Ok(entries) = std::fs::read_dir(self.dir(connection_id)) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if Self::number(file_name).is_none() {
                continue;
            }
            let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            let preview = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string))
                .unwrap_or_default();
            out.push(ScriptFile {
                name: file_name.trim_end_matches(".sql").to_string(),
                connection_id: connection_id.to_string(),
                path,
                modified,
                preview,
            });
        }
    }

    pub fn list(&self, connection_id: Option<&str>) -> Vec<ScriptFile> {
        let mut out = Vec::new();
        match connection_id {
            Some(id) => self.list_dir(id, &mut out),
            None => {
                for entry in std::fs::read_dir(&self.root).into_iter().flatten().flatten() {
                    if entry.path().is_dir() {
                        if let Some(id) = entry.file_name().to_str() {
                            self.list_dir(id, &mut out);
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.name.cmp(&a.name)));
        out
    }

    pub fn most_recent(&self, connection_id: &str) -> Option<ScriptFile> {
        self.list(Some(connection_id)).into_iter().next()
    }

    pub fn create(&self, connection_id: &str) -> Result<ScriptFile> {
        let dir = self.dir(connection_id);
        std::fs::create_dir_all(&dir).with_context(|| format!("Cannot create {}", dir.display()))?;
        let next = std::fs::read_dir(&dir)?
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(Self::number))
            .max()
            .unwrap_or(0)
            + 1;
        let name = format!("{PREFIX}{next}");
        let path = dir.join(format!("{name}.sql"));
        std::fs::write(&path, "").with_context(|| format!("Cannot create {}", path.display()))?;
        Ok(ScriptFile {
            path,
            connection_id: connection_id.to_string(),
            name,
            modified: SystemTime::now(),
            preview: String::new(),
        })
    }

    pub fn read(&self, script: &ScriptFile) -> Result<String> {
        std::fs::read_to_string(&script.path).with_context(|| format!("Cannot read {}", script.path.display()))
    }

    pub fn write(&self, path: &Path, text: &str) -> Result<()> {
        std::fs::write(path, text).with_context(|| format!("Cannot save {}", path.display()))
    }
}

pub fn filter_scripts<'a>(scripts: &'a [ScriptFile], query: &str) -> Vec<&'a ScriptFile> {
    let query = query.trim().to_lowercase();
    scripts
        .iter()
        .filter(|s| query.is_empty() || s.name.to_lowercase().contains(&query) || s.preview.to_lowercase().contains(&query))
        .collect()
}
```

(`platform-hal` is already an app dependency; check its module path with `grep -n "pub mod" crates/platform-hal/src/lib.rs` and adjust `platform_hal::paths::config_dir` if it is re-exported differently.)

- [ ] **Step 4: Run tests** — `cargo test -p fastdb-app script_store` → 4 PASS.
- [ ] **Step 5: Commit** — `feat(app): store SQL scripts as numbered files per connection`

---

### Task 2: Script tabs backed by files

**Files:** `query.rs`, `mod.rs`, `sql_script.rs`, `assets.rs`, `top_bar.rs`, `tab_context_menu.rs`, `keyboard.rs`, `nav_context_menu.rs`, `script_view.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces:
  - `TabState` fields `script_path: Option<PathBuf>`, `connection_id: Option<String>`, `saved_text: String` (text last written to the file)
  - `TabState::script(id: u64, script: &ScriptFile, text: &str, theme: Theme, cache: Arc<SchemaCache>) -> Self` — title = `script.name`, editor holds `text` with the caret at the end, `saved_text = text`
  - `TabState::is_dirty(&self) -> bool` — script tab whose editor text differs from `saved_text`
  - `Workspace::open_sql_script(&mut self, conn_id: Option<&str>, cx: &mut Context<Self>)` — spec item 1 (the chooser branch calls `self.open_script_chooser(conn_id)` from Task 3; until then it opens a new script)
  - `Workspace::open_script_file(&mut self, script: &ScriptFile, cx)` — activates the tab showing it, else reads it into a new tab; then `activate_script_connection`
  - `Workspace::activate_script_connection(&mut self, cx)` — if the active tab is a script tab whose `connection_id` differs from `active_connection_id`, `connect_saved_by_id(id, cx)`
  - `Workspace::save_script_tab(&mut self, idx: usize)` and `save_dirty_scripts(&mut self)` — write dirty tabs, update `saved_text`, put write errors into `last_error`

- [ ] **Step 1: Failing tests** (in `assets.rs`; update the two existing script tests to the new constructor)

```rust
    fn sample_script_tab(text: &str) -> crate::workspace::TabState {
        use crate::workspace::{ScriptFile, TabState};
        use ui_core::theme::Theme;
        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        let script = ScriptFile {
            path: std::path::PathBuf::from("/tmp/Script-4.sql"),
            connection_id: "pg".to_string(),
            name: "Script-4".to_string(),
            modified: std::time::SystemTime::now(),
            preview: String::new(),
        };
        TabState::script(3, &script, text, Theme::dark(), cache)
    }

    #[test]
    fn test_script_tab_title_text_and_dirty_flag_come_from_its_file() {
        let mut tab = sample_script_tab("SELECT 1;");
        assert_eq!(tab.title, "Script-4");
        assert_eq!(tab.connection_id.as_deref(), Some("pg"));
        assert_eq!(tab.editor.buffer.text(), "SELECT 1;");
        assert!(!tab.is_dirty());
        tab.editor.insert_text("\nSELECT 2;");
        assert!(tab.is_dirty());
        assert!(!sample_table_tab().is_dirty());
    }
```

Replace `TabState::script(3, "SQL Script 1".to_string(), …)` in `test_script_tabs_start_empty_with_the_editor_focused` and `test_script_fetch_query_pages_selects_and_runs_other_statements_as_written` with `sample_script_tab("")`.

- [ ] **Step 2: Run** — `cargo test -p fastdb-app test_script` → compile errors.

- [ ] **Step 3: Implement**
  1. `query.rs`: add the three fields (doc comments) and defaults in `blank`; new `script` constructor; `is_dirty`:
     ```rust
     pub fn is_dirty(&self) -> bool {
         self.script_path.is_some() && self.editor.buffer.text() != self.saved_text
     }
     ```
  2. `mod.rs`: field `pub script_store: ScriptStore` (init `ScriptStore::new(ScriptStore::default_root())`), remove `next_script_number`; in the caret-blink loop call `this.save_dirty_scripts();` before `cx.notify()`.
  3. `sql_script.rs`: delete `script_title` and its test.
  4. `query.rs`: replace `open_sql_script` with the spec-1 logic:
     ```rust
     pub fn open_sql_script(&mut self, conn_id: Option<&str>, cx: &mut Context<Self>) {
         let Some(conn_id) = conn_id.map(str::to_string).or_else(|| self.active_connection_id.clone()) else {
             self.show_connection_modal = true;
             return;
         };
         let script = match self.script_store.most_recent(&conn_id) {
             Some(recent) if self.tabs.iter().any(|t| t.script_path.as_deref() == Some(recent.path.as_path())) => {
                 self.open_script_chooser(&conn_id); // Task 3; until then: create a new script
                 return;
             }
             Some(recent) => recent,
             None => match self.script_store.create(&conn_id) {
                 Ok(s) => s,
                 Err(e) => { tracing::warn!("{e:#}"); return; }
             },
         };
         self.open_script_file(&script, cx);
     }
     ```
     `open_script_file`: find a tab with that `script_path` → `active_tab = idx`; else `script_store.read` (on error push nothing and log) and push `TabState::script(...)`; then `self.activate_script_connection(cx)`.
     `save_script_tab(idx)`: if dirty, `script_store.write(path, &text)`; on Ok set `saved_text`, on Err set `last_error = Some(format!("{e:#}"))`. `save_dirty_scripts` loops over all tabs.
     In `run_active_query`, call `self.save_script_tab(self.active_tab)` for script tabs before running.
  5. `tab_context_menu.rs::close_tabs_where`: before `retain`, call `self.save_script_tab(i)` for every index that will be closed. Then call `activate_script_connection` is not possible without `cx` — instead mark nothing; the next click on a tab switches.
  6. `top_bar.rs`: tab `on_click` calls `this.activate_script_connection(cx)` after setting `active_tab`; "+ SQL" calls `this.open_sql_script(None, cx)`.
  7. `keyboard.rs`: Cmd+] → `self.open_sql_script(None, cx)`; Cmd+Q → `self.save_dirty_scripts();` before `cx.quit()`.
  8. `nav_context_menu.rs`: `OpenSqlEditor` → `self.open_sql_script(Some(conn_id), cx)` (drop the explicit `connect_saved_by_id`, `open_script_file` does it).
  9. `script_view.rs` toolbar: after the hint text, a right-aligned label with `icons/database.svg` and the connection's `name` looked up in `saved_connections` by `tab.connection_id`.

- [ ] **Step 4: Run** — `cargo test -p fastdb-app` all PASS; `cargo build -p fastdb-app` without warnings.
- [ ] **Step 5: Commit** — `feat(app): keep SQL scripts in files per connection and reopen the latest one`

---

### Task 3: "Choose SQL script" dialog

**Files:** Create `script_chooser.rs`; modify `mod.rs`, `keyboard.rs`, `query.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub struct ScriptChooser {
      /// Connection the dialog was opened for
      pub connection_id: String,
      pub query: String,
      pub caret: usize,
      pub anchor: Option<usize>,
      pub history: LineHistory,
      pub all_connections: bool,
      pub selected: usize,
      /// Scripts listed for the current checkbox state, newest first
      pub scripts: Vec<ScriptFile>,
  }
  impl ScriptChooser {
      /// Indices into `scripts` matching `query`
      pub fn visible(&self) -> Vec<usize>;
      pub fn move_selection(&mut self, down: bool);
      /// Script under the selection, if any
      pub fn selected_script(&self) -> Option<&ScriptFile>;
  }
  impl Workspace {
      pub(super) fn open_script_chooser(&mut self, conn_id: &str);
      pub(super) fn handle_script_chooser_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>);
      pub(super) fn render_script_chooser(&mut self, cx: &mut Context<Self>) -> impl IntoElement;
  }
  ```
- Consumes: Task 1 `filter_scripts`, Task 2 `open_script_file`, `text_input::{handle_line_key, LineInput, render_field_content}`.

- [ ] **Step 1: Failing tests** (pure state, in `script_chooser.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::ScriptChooser;
    use crate::workspace::ScriptFile;
    use std::time::SystemTime;

    fn script(name: &str, preview: &str) -> ScriptFile {
        ScriptFile {
            path: format!("/tmp/{name}.sql").into(),
            connection_id: "pg".into(),
            name: name.into(),
            modified: SystemTime::now(),
            preview: preview.into(),
        }
    }

    fn chooser() -> ScriptChooser {
        ScriptChooser::new("pg".into(), vec![script("Script-3", "explain x"), script("Script-2", "select 1"), script("Script-1", "")])
    }

    #[test]
    fn selection_moves_within_the_filtered_list() {
        let mut c = chooser();
        assert_eq!(c.selected_script().unwrap().name, "Script-3");
        c.move_selection(true);
        c.move_selection(true);
        c.move_selection(true);
        assert_eq!(c.selected_script().unwrap().name, "Script-1");
        c.move_selection(false);
        assert_eq!(c.selected_script().unwrap().name, "Script-2");
    }

    #[test]
    fn typing_a_query_filters_and_resets_the_selection() {
        let mut c = chooser();
        c.move_selection(true);
        c.set_query("SELECT");
        assert_eq!(c.visible(), vec![1]);
        assert_eq!(c.selected_script().unwrap().name, "Script-2");
        c.set_query("zzz");
        assert!(c.selected_script().is_none());
    }
}
```

with `ScriptChooser::new(connection_id, scripts)` (selected 0, empty query, `all_connections: false`) and `set_query(&mut self, query: &str)` (sets `query`, `caret = usize::MAX`, `selected = 0`).

- [ ] **Step 2: Run** — FAIL (missing items).
- [ ] **Step 3: Implement state** — `visible` = `filter_scripts` mapped back to indices; `move_selection` clamps within `visible().len()`; `selected_script` = `scripts[visible()[selected]]`.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Wire it**
  - `mod.rs`: `pub script_chooser: Option<ScriptChooser>` (init `None`); render `self.render_script_chooser(cx)` like the connection dialog when `Some`.
  - `open_script_chooser(conn_id)`: `self.script_chooser = Some(ScriptChooser::new(conn_id.into(), self.script_store.list(Some(conn_id))))`.
  - `keyboard.rs`: right after the Cmd+W intercept, `if self.script_chooser.is_some() { self.handle_script_chooser_key(&event.keystroke, cx); return; }`. Handling: `escape` closes; `up`/`down` move; `enter` opens the selected script (`take()` the chooser, then `open_script_file`); otherwise feed `handle_line_key` with a `LineInput` over `query`/`caret`/`anchor`/`history` and, if the text changed, `set_query(&query)` to reset the selection.
  - `render_script_chooser`: modal like the connection dialog (dimmed backdrop, `occlude`, `on_mouse_down_out` closes), 560 px wide, title `Choose SQL script for '<name>'`; search box via `render_field_content` with placeholder "Enter a part of script name here"; row with a "New script" button (`script_store.create(conn)` then `open_script_file`, closes the dialog) and a checkbox "Show scripts for all connections" (toggles `all_connections`, reloads `scripts` with `list(None)` or `list(Some(conn))`, resets selection); a header row (Script / Time / Info / Connection when all) and up to 14 visible rows from `visible()`, the selected one highlighted, rows `on_click` open the script on `click_count >= 2` and select it otherwise. Time uses `chrono::DateTime::<chrono::Local>::from(script.modified).format("%Y-%m-%d %H:%M:%S")`. Connection name comes from `saved_connections`.
  - `query.rs::open_sql_script`: the "already open" branch calls `self.open_script_chooser(&conn_id)`.
- [ ] **Step 6: Run** — `cargo test -p fastdb-app` PASS, build clean.
- [ ] **Step 7: Commit** — `feat(app): choose a saved SQL script when the latest one is already open`

---

### Task 4: Verify in the app and document

- [ ] **Step 1: Manual check** (`SDKROOT=$(xcrun --show-sdk-path) ./ci/build-macos.sh`, open `dist/macos/FastDB.app`)
  1. Connect to A; "+ SQL" opens `Script-1`; type; close the tab; "+ SQL" reopens `Script-1` with the text.
  2. With `Script-1` open, "+ SQL" shows the chooser; "New script" opens `Script-2`.
  3. Quit with Cmd+Q and relaunch: scripts and text are kept; numbering continues.
  4. Connect to B too; "SQL Editor" on B opens B's script and switches the active connection; clicking A's script tab switches back; Cmd+Enter runs on the tab's connection.
  5. Chooser: search filters, ↑/↓/Enter, Esc, "Show scripts for all connections" shows both with a Connection column.
- [ ] **Step 2: README** — SQL editor section: scripts saved to `<config dir>/scripts/<connection id>/Script-<n>.sql`, "+ SQL" behaviour, chooser; update the "Not built yet" extras (rename/delete scripts).
- [ ] **Step 3:** `cargo test --workspace` PASS, commit `docs: describe saved SQL scripts`, merge into `main`.
