use db_wire::{ColumnMeta, DatabaseDriver, DbDriverKind, StreamEvent, TableSummary, create_driver};
use gpui::*;
use mem_engine::DataType;
use sql_intel::SchemaCache;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use ui_core::theme::Theme;
use ui_editor::SqlEditorView;
use ui_grid::VirtualDataGrid;

mod about_dialog;
mod column_menu;
mod connection_config;
mod connection_dialog;
mod connection_state;
mod connections;
use connection_state::{ConnectionFormState, ConnectionManager, ConnectionSession};
mod data_edit;
mod data_panel;
mod ddl;
mod diagram_panel;
mod fetch_plan;
mod filter_suggest;
mod keyboard;
mod nav_context_menu;
mod navigator;
mod operations;
mod panel_resize;
mod panes;
mod properties_panel;
mod query;
mod script_tabs;
mod tab_state;
use operations::{OperationKind, Operations};
mod script_chooser;
mod script_store;
mod script_view;
mod sql_script;
mod tab_context_menu;
mod table_view;
mod text_input;
mod top_bar;
mod update_dialog;
mod value_format;
mod value_panel;
mod widgets;

use crate::updater::{
    DEFAULT_REPO, ReleaseInfo, UpdateCheckResult, UpdateState, check_github_latest_release,
};
pub use column_menu::ColumnMenu;
pub use connection_config::SavedConnection;
use db_wire::sql::generate_table_ddl;
use ddl::render_dbeaver_col_badge;
pub use filter_suggest::FilterSuggest;
pub use nav_context_menu::NavContextMenu;
use panel_resize::{CENTER_CHROME_WIDTH, NAVIGATOR_DEFAULT_WIDTH, PanelDrag, PanelSplitter};
pub use panes::Panes;
pub use script_chooser::ScriptChooser;
pub use script_store::{ScriptFile, ScriptStore, filter_scripts};
pub use tab_context_menu::TabContextMenu;
#[cfg(test)]
pub use tab_state::{QueryStatus, script_fetch_query};
pub use tab_state::{TabState, group_thousands};
use text_input::{
    CaretPick, LineHistory, LineInput, apply_pick, handle_line_key, render_caret_text,
    selection_range,
};
use ui_core::actions::CheckForUpdates;
pub use update_dialog::{DownloadUiState, UpdateDialog, UpdateDialogStatus};
use value_panel::{VALUE_PANEL_WIDTH, ValuePanelState};
use widgets::{dbeaver_driver_icon, render_field_content};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableSubTab {
    Properties,
    Data,
    Diagram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertySection {
    Columns,
    Constraints,
    ForeignKeys,
    Indexes,
    Dependencies,
    References,
    Partitions,
    Triggers,
    Rules,
    Policies,
    Statistics,
    Permissions,
    Ddl,
    Virtual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataViewMode {
    Grid,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormField {
    Name,
    Host,
    Port,
    Database,
    User,
    Password,
    TlsCa,
}

pub struct Workspace {
    form: ConnectionFormState,
    connections: ConnectionManager,
    pub theme: Theme,
    pub focus_handle: FocusHandle,
    pub active_tab: usize,
    pub tabs: Vec<TabState>,
    operations: Operations,
    pub next_tab_id: u64,
    /// Saved SQL scripts, one folder per connection
    pub script_store: ScriptStore,
    /// "Choose SQL script" dialog, if open
    pub script_chooser: Option<ScriptChooser>,

    // Real Database Connection Setup State
    /// Open connections other than the active one, kept alive for instant switching
    /// Undo history of each connection form field, reset whenever the form is opened
    // DBeaver-style Tree Navigator State
    pub resizing_column: Option<(usize, usize, f32, f32)>, // (tab_idx, col_idx, start_mouse_x, start_col_width)
    pub resizing_panel: Option<PanelDrag>,
    /// Live widths of the side panels, changed by dragging their splitters
    pub navigator_width: f32,
    pub value_panel_width: f32,
    pub is_filter_focused: bool,
    pub show_right_panels: bool,
    pub value_panel: ValuePanelState,
    pub cursor_visible: bool,
    /// Selection anchor of the active form field (see `selection_range`)
    /// Selection anchor of the filter bar
    pub filter_anchor: Option<usize>,
    /// Caret of the active form field, as a char index (clamped; `usize::MAX` = end)
    /// Caret of the filter bar, as a char index (clamped; `usize::MAX` = end)
    pub filter_cursor: usize,
    /// Column name suggestions under the filter bar, if open
    pub filter_suggest: Option<FilterSuggest>,
    pub is_fetch_size_focused: bool,
    /// Selection anchor of the fetch size box
    pub fetch_size_anchor: Option<usize>,
    /// Caret of the fetch size box, as a char index (clamped; `usize::MAX` = end)
    pub fetch_size_cursor: usize,
    /// Undo history of the text of the grid cell being edited, reset for each cell
    pub cell_edit_history: LineHistory,
    /// Right-click menu of a navigator connection, if open
    pub nav_context_menu: Option<NavContextMenu>,
    /// Right-click menu of a top bar tab, if open
    pub tab_context_menu: Option<TabContextMenu>,
    /// Dropdown of a result grid column header, if open
    pub column_menu: Option<ColumnMenu>,
    /// Cached views of the window regions, created on the first render
    pub panes: Option<Panes>,
    /// HTTP client for checking updates and downloading assets
    pub http_client: reqwest::Client,
    /// Preferences and cached timestamp for update checks
    pub update_state: UpdateState,
    /// Active Software Update modal dialog, if open
    pub update_dialog: Option<UpdateDialog>,
    pub show_about_dialog: bool,
    /// Discovered update info waiting to be applied/viewed
    pub pending_update: Option<ReleaseInfo>,
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let theme = Theme::dark();
        let focus_handle = cx.focus_handle();

        let (saved_connections, active_connection_id) =
            connection_config::ConnectionRepository::new(
                platform_hal::paths::config_dir().join("connections.json"),
            )
            .load_saved_connections();
        let initial_active_conn = if let Some(ref id) = active_connection_id {
            saved_connections.iter().find(|c| &c.id == id).cloned()
        } else {
            saved_connections.first().cloned()
        };

        let workspace = Self {
            theme,
            focus_handle,
            active_tab: 0,
            tabs: Vec::new(),
            operations: Operations::default(),
            next_tab_id: 1,
            script_store: ScriptStore::new(ScriptStore::default_root()),
            script_chooser: None,

            // Connection Setup Form Defaults
            resizing_column: None,
            resizing_panel: None,
            navigator_width: NAVIGATOR_DEFAULT_WIDTH,
            value_panel_width: VALUE_PANEL_WIDTH,
            is_filter_focused: false,
            show_right_panels: false,
            value_panel: ValuePanelState::default(),
            cursor_visible: true,
            filter_anchor: None,
            filter_cursor: usize::MAX,
            filter_suggest: None,
            is_fetch_size_focused: false,
            fetch_size_anchor: None,
            fetch_size_cursor: usize::MAX,
            cell_edit_history: LineHistory::new(),
            nav_context_menu: None,
            tab_context_menu: None,
            column_menu: None,
            panes: None,
            http_client: reqwest::Client::new(),
            update_state: UpdateState::load(),
            update_dialog: None,
            show_about_dialog: false,
            pending_update: None,
            form: ConnectionFormState::from_saved(initial_active_conn.as_ref()),
            connections: ConnectionManager::new(saved_connections, active_connection_id),
        };

        // Automatic background update check on app startup (throttled to once a day)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if workspace.update_state.is_time_for_auto_check(now) {
            cx.spawn(async move |this, cx| {
                // Wait 3 seconds so main window is displayed first
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                let _ = this.update(cx, |workspace, cx| {
                    workspace.trigger_update_check(false, cx);
                });
            })
            .detach();
        }

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(530))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.cursor_visible = !this.cursor_visible;
                        this.save_dirty_scripts();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        workspace
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let panes = self.panes.get_or_insert_with(|| Panes::new(cx)).clone();

        if self.form.show_connection_modal {
            window.focus(&self.focus_handle);
        }

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg_app)
            .text_color(theme.text_primary)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| {
                this.trigger_update_check(true, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.handle_key_down(event, cx);
            }));

        if self.resizing_column.is_some() {
            root = root
                .cursor_col_resize()
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                    if let Some((tab_idx, col_idx, start_x, start_w)) = this.resizing_column {
                        let current_x = f32::from(event.position.x);
                        let delta = current_x - start_x;
                        let new_w = (start_w + delta).max(ui_grid::MIN_COL_WIDTH);
                        if let Some(tab) = this.tabs.get_mut(tab_idx) {
                            tab.grid.set_column_width(col_idx, new_w);
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        if this.resizing_column.is_some() {
                            this.resizing_column = None;
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        if this.resizing_column.is_some() {
                            this.resizing_column = None;
                            cx.notify();
                        }
                    }),
                );
        }

        if self.resizing_panel.is_some() {
            root = root
                .cursor_col_resize()
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                    this.drag_panel_splitter(
                        f32::from(event.position.x),
                        f32::from(window.viewport_size().width),
                    );
                    cx.notify();
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        this.resizing_panel = None;
                        cx.notify();
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        this.resizing_panel = None;
                        cx.notify();
                    }),
                );
        }

        // 1. Top Bar / Tabs & Toolbar
        root = root.child(panes.top_bar());

        // 2. Main Body: Left Sidebar + Center Workspace
        root = root.child(
            div()
                .flex()
                .flex_1()
                .w_full()
                .overflow_hidden()
                .child(panes.navigator(self.navigator_width))
                .child(panes.table_view()),
        );

        if let Some(menu) = self.nav_context_menu.clone() {
            root = root.child(self.render_nav_context_menu(menu, cx));
        }
        if let Some(menu) = self.tab_context_menu.clone() {
            root = root.child(self.render_tab_context_menu(menu, cx));
        }
        if let Some(menu) = self.column_menu.clone() {
            root = root.child(self.render_column_menu(menu, cx));
        }

        // 4. Modal Dialog: Database Connection Setup
        if self.form.show_connection_modal {
            root = root.child(self.render_connection_dialog(cx));
        }
        if self.script_chooser.is_some() {
            root = root.child(self.render_script_chooser(cx));
        }
        if self.update_dialog.is_some() {
            root = root.child(self.render_update_dialog(cx));
        }
        if self.show_about_dialog {
            root = root.child(self.render_about_dialog(cx));
        }

        root
    }
}

impl Workspace {
    /// Triggers checking GitHub for a new release
    pub fn trigger_update_check(&mut self, is_manual: bool, cx: &mut Context<Self>) {
        self.update_state.record_check_now();
        if is_manual {
            self.update_dialog = Some(UpdateDialog::checking(true));
            cx.notify();
        }

        let http_client = self.http_client.clone();
        let state = self.update_state.clone();
        let repo = DEFAULT_REPO.to_string();
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let (tx, rx) = flume::bounded::<UpdateCheckResult>(1);

        let executor = cx.background_executor().clone();
        cx.background_executor()
            .spawn(async move {
                let start = std::time::Instant::now();
                let res = match check_github_latest_release(
                    &http_client,
                    &repo,
                    &current_version,
                    &state,
                    is_manual,
                )
                .await
                {
                    Ok(r) => r,
                    Err(e) => UpdateCheckResult::Error(e.to_string()),
                };

                // Keep dialog in loading state for at least 800ms so the user sees the check in progress
                if is_manual {
                    let elapsed = start.elapsed();
                    let min_dur = std::time::Duration::from_millis(800);
                    if elapsed < min_dur {
                        executor.timer(min_dur - elapsed).await;
                    }
                }

                let _ = tx.send(res);
            })
            .detach();

        cx.spawn(async move |this, cx| {
            if let Ok(res) = rx.recv_async().await {
                let _ = this.update(cx, |workspace, cx| match res {
                    UpdateCheckResult::UpdateAvailable(info) => {
                        workspace.pending_update = Some(*info.clone());
                        workspace.update_dialog =
                            Some(UpdateDialog::update_available(*info, is_manual));
                        cx.notify();
                    }
                    UpdateCheckResult::UpToDate { current_version } => {
                        if is_manual {
                            workspace.update_dialog =
                                Some(UpdateDialog::up_to_date(current_version, true));
                            cx.notify();
                        }
                    }
                    UpdateCheckResult::Skipped(info) => {
                        if is_manual {
                            workspace.pending_update = Some(*info.clone());
                            workspace.update_dialog =
                                Some(UpdateDialog::update_available(*info, true));
                            cx.notify();
                        }
                    }
                    UpdateCheckResult::Error(err) => {
                        if is_manual {
                            workspace.update_dialog = Some(UpdateDialog::error(err, true));
                            cx.notify();
                        } else {
                            tracing::warn!("Auto update background check failed: {}", err);
                        }
                    }
                });
            }
        })
        .detach();
    }
}

impl Workspace {
    fn tab_session(&self, index: usize) -> &ConnectionSession {
        self.tabs
            .get(index)
            .and_then(|t| t.connection_id.as_ref())
            .map_or_else(
                || self.connections.active(),
                |id| self.connections.session(id),
            )
    }
    fn tab_provider(&self, index: usize) -> DbDriverKind {
        self.tab_session(index).kind
    }
}
