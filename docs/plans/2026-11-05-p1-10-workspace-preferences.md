# P1-10 workspace preferences and session restoration

## Scope and Contract

Implement the complete P1-10 roadmap requirements from `docs/ROADMAP.md`:
1. **Restore open tabs & layout and unsaved editor state after restart**:
   - Reopen open tabs in the exact order with their connection, table/schema, filter text, subtab, and property section.
   - Restore script tabs with their connection identity, path, and unsaved editor buffer state.
   - Acceptance safety gate: never replay SQL queries on restore or pretend to restore a live/active transaction. Table and script tabs open in an idle, clean state.
   - Restore workspace layout: navigator width, value panel width, and right-panel visibility.
2. **Configurable shortcuts, fonts, theme**:
   - Theme toggle (Dark / Light) with live propagation across all views, editors, grids, and dialogs.
   - Editor font size (with zoom in/out shortcuts `Cmd+=` / `Cmd+-` / `Cmd+0`) and configurable font family.
   - Configurable shortcuts / keymap with defaults and custom overrides.
3. **Persisted preferences**:
   - Versioned, backward-compatible JSON storage in `config_dir()`:
     - `preferences.json`: user settings (theme, font size/family, default fetch size, keymap overrides).
     - `workspace-state.json`: runtime session state (open tabs, active tab index, panel dimensions).
   - Atomic writes via temporary files and robust fallback on corrupted or missing files.

## Technical Architecture

### 1. Storage & Serialization (`crates/app/src/workspace/preferences.rs`)
- `Preferences`:
  - `version`: u32 (1)
  - `theme`: `ThemeMode` (`Dark`, `Light`)
  - `editor_font_size`: f32 (default 13.0, clamped [9.0, 32.0])
  - `editor_font_family`: Option<String>
  - `default_fetch_size`: usize (default 200)
  - `keymap`: HashMap<String, String> (action name -> keystroke)
- `WorkspaceState`:
  - `version`: u32 (1)
  - `active_tab`: usize
  - `navigator_width`: f32
  - `value_panel_width`: f32
  - `show_right_panels`: bool
  - `tabs`: Vec<PersistedTab>
    - `Table { connection_id: Option<String>, schema: String, table: String, filter_sql: String, active_subtab: TableSubTab, active_property_section: PropertySection, fetch_size: usize }`
    - `Script { connection_id: Option<String>, title: String, script_path: Option<PathBuf>, unsaved_text: Option<String>, editor_focused: bool }`

### 2. Editor & Layout Font Configuration (`crates/ui-editor/`)
- Extend `SqlEditorView` and `EditorLayout` to respect configurable `font_size` and optional `font_family`.
- Maintain line height calculation relative to font size.

### 3. Theme Propagation (`crates/app/src/workspace/`)
- Add `ToggleTheme` action in `ui-core/actions.rs` and menu item in the application menu.
- Method `workspace.toggle_theme(cx)`:
  - Switches between `Theme::dark()` and `Theme::light()`.
  - Propagates theme to all open tabs (`tab.editor.theme`, `tab.grid.theme`), modals, and pane views.
  - Persists preference change to `preferences.json`.

### 4. Session Restoration Lifecycle
- **On Startup (`Workspace::new`)**:
  - Load `preferences.json` (apply theme, font size, default fetch size).
  - Load `workspace-state.json`.
  - Apply layout dimensions.
  - Reopen tabs:
    - Table tabs: resolve against valid saved connections; populate schema cache and table grid in an idle state without triggering SQL execution.
    - Script tabs: load script text from disk or restored unsaved draft; initialize tab session with fresh idle state.
  - Activate saved `active_tab`.
- **On Save / Exit / Tab Operations**:
  - Save `workspace-state.json` when tabs are opened, closed, switched, or on app quit/restart.
  - Save `preferences.json` when theme or settings change.

## Verification & Acceptance
- Unit and integration tests covering:
  - Serialization/deserialization of preferences and workspace state with corrupt/missing fallback.
  - Tab restoration without SQL execution or transaction resurrection.
  - Unsaved draft preservation.
  - Theme toggling and propagation.
  - Editor font sizing bounds and layout calculations.
- Code quality gates:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets --locked`
  - `cargo test --workspace --locked`
  - `cargo build -p fastdb-app --locked`
