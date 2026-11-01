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
mod properties;
mod properties_panel;
mod query;
mod script_tabs;
mod tab_state;
use operations::{OperationKind, Operations};
mod script_chooser;
mod script_saves;
mod script_store;
use script_saves::{AfterSave, SaveQueue};
mod script_view;
mod software_update;
mod sql_script;
mod tab_context_menu;
mod table_view;
mod text_input;
mod top_bar;
use software_update::UpdateController;
mod value_format;
mod value_panel;
mod widgets;
mod workspace_state;
use workspace_state::{WorkspaceInput, WorkspaceLayout, WorkspaceMenus};

pub use column_menu::ColumnMenu;
pub use connection_config::SavedConnection;
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
    menus: WorkspaceMenus,
    layout: WorkspaceLayout,
    input: WorkspaceInput,
    form: ConnectionFormState,
    connections: ConnectionManager,
    pub theme: Theme,
    pub focus_handle: FocusHandle,
    pub active_tab: usize,
    pub tabs: Vec<TabState>,
    tab_scroll: ScrollHandle,
    revealed_tab: Option<(u64, usize, usize)>,
    operations: Operations,
    pub next_tab_id: u64,
    /// Saved SQL scripts, one folder per connection
    pub script_store: ScriptStore,
    save_queue: SaveQueue,
    script_request_id: u64,
    pub(super) allow_quit: bool,
    /// "Choose SQL script" dialog, if open
    pub script_chooser: Option<ScriptChooser>,

    pub value_panel: ValuePanelState,
    /// Cached views of the window regions, created on the first render
    pub panes: Option<Panes>,
    updates: Entity<UpdateController>,
    _update_changed: Subscription,
    _restart_requested: Subscription,
    pub show_about_dialog: bool,
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

        let save_queue = SaveQueue::default();
        let updates = cx.new(|_| UpdateController::new(theme.clone(), save_queue.clone()));
        let mut signature = updates.read(cx).signature();
        let update_changed = cx.observe(&updates, move |_, update, cx| {
            let next = update.read(cx).signature();
            if next != signature {
                signature = next;
                cx.notify();
            }
        });
        let restart_requested = cx.subscribe(
            &updates,
            |this, _, _: &software_update::RestartRequested, cx| {
                this.save_then(
                    this.tabs.iter().map(|t| t.id).collect(),
                    AfterSave::Restart,
                    cx,
                );
            },
        );
        let workspace = Self {
            theme,
            focus_handle,
            active_tab: 0,
            tabs: Vec::new(),
            tab_scroll: ScrollHandle::new(),
            revealed_tab: None,
            operations: Operations::default(),
            next_tab_id: 1,
            script_store: ScriptStore::new(ScriptStore::default_root()),
            save_queue,
            updates,
            _update_changed: update_changed,
            _restart_requested: restart_requested,
            script_request_id: 0,
            allow_quit: false,
            script_chooser: None,

            value_panel: ValuePanelState::default(),
            panes: None,
            show_about_dialog: false,
            form: ConnectionFormState::from_saved(initial_active_conn.as_ref()),
            connections: ConnectionManager::new(saved_connections, active_connection_id),
            input: WorkspaceInput {
                is_filter_focused: false,
                cursor_visible: true,
                filter_anchor: None,
                filter_cursor: usize::MAX,
                filter_suggest: None,
                is_fetch_size_focused: false,
                fetch_size_anchor: None,
                fetch_size_cursor: usize::MAX,
                cell_edit_history: LineHistory::new(),
            },
            layout: WorkspaceLayout {
                resizing_column: None,
                resizing_panel: None,
                navigator_width: NAVIGATOR_DEFAULT_WIDTH,
                value_panel_width: VALUE_PANEL_WIDTH,
                show_right_panels: false,
            },
            menus: WorkspaceMenus {
                nav_context_menu: None,
                tab_context_menu: None,
                column_menu: None,
            },
        };

        // Automatic background update check on app startup (throttled to once a day)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if workspace.updates.read(cx).is_time_for_auto_check(now) {
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
                        let has_cursor = this.form.show_connection_modal
                            || this.script_chooser.is_some()
                            || this.input.is_filter_focused
                            || this.input.is_fetch_size_focused
                            || this.tabs.get(this.active_tab).is_some_and(|tab| {
                                (tab.is_script() && tab.editor_focused)
                                    || tab.grid.editing_cell.is_some()
                            });
                        if !has_cursor || this.updates.read(cx).is_open() {
                            return;
                        }
                        this.input.cursor_visible = !this.input.cursor_visible;
                        if this.form.show_connection_modal || this.script_chooser.is_some() {
                            cx.notify();
                        } else if let Some(panes) = &this.panes {
                            panes.notify_table(cx);
                        }
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

        if self.layout.resizing_column.is_some() {
            root = root
                .cursor_col_resize()
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                    if let Some((tab_idx, col_idx, start_x, start_w)) = this.layout.resizing_column
                    {
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
                        if this.layout.resizing_column.is_some() {
                            this.layout.resizing_column = None;
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        if this.layout.resizing_column.is_some() {
                            this.layout.resizing_column = None;
                            cx.notify();
                        }
                    }),
                );
        }

        if self.layout.resizing_panel.is_some() {
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
                        this.layout.resizing_panel = None;
                        cx.notify();
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        this.layout.resizing_panel = None;
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
                .child(panes.navigator(self.layout.navigator_width))
                .child(panes.table_view()),
        );

        if let Some(menu) = self.menus.nav_context_menu.clone() {
            root = root.child(self.render_nav_context_menu(menu, cx));
        }
        if let Some(menu) = self.menus.tab_context_menu.clone() {
            root = root.child(self.render_tab_context_menu(menu, cx));
        }
        if let Some(menu) = self.menus.column_menu.clone() {
            root = root.child(self.render_column_menu(menu, cx));
        }

        // 4. Modal Dialog: Database Connection Setup
        if self.form.show_connection_modal {
            root = root.child(self.render_connection_dialog(cx));
        }
        if self.script_chooser.is_some() {
            root = root.child(self.render_script_chooser(cx));
        }
        if self.updates.read(cx).is_open() {
            root = root.child(self.updates.clone());
        }
        if self.show_about_dialog {
            root = root.child(self.render_about_dialog(cx));
        }

        root
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

impl Workspace {
    pub fn trigger_update_check(&mut self, manual: bool, cx: &mut Context<Self>) {
        self.updates
            .update(cx, |updates, cx| updates.trigger_update_check(manual, cx));
    }
}

impl Workspace {
    fn notify_result(&self, cx: &mut Context<Self>) {
        if let Some(panes) = &self.panes {
            panes.notify_table(cx);
        } else {
            cx.notify();
        }
    }
}
