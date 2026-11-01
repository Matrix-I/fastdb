use super::{ColumnMenu, FilterSuggest, LineHistory, NavContextMenu, PanelDrag, TabContextMenu};
pub(super) struct WorkspaceInput {
    pub is_filter_focused: bool,
    pub cursor_visible: bool,
    pub filter_anchor: Option<usize>,
    pub filter_cursor: usize,
    pub filter_suggest: Option<FilterSuggest>,
    pub is_fetch_size_focused: bool,
    pub fetch_size_anchor: Option<usize>,
    pub fetch_size_cursor: usize,
    pub cell_edit_history: LineHistory,
}
pub(super) struct WorkspaceLayout {
    pub resizing_column: Option<(usize, usize, f32, f32)>, // (tab_idx, col_idx, start_mouse_x, start_col_width)
    pub resizing_panel: Option<PanelDrag>,
    pub navigator_width: f32,
    pub value_panel_width: f32,
    pub show_right_panels: bool,
}
pub(super) struct WorkspaceMenus {
    pub nav_context_menu: Option<NavContextMenu>,
    pub tab_context_menu: Option<TabContextMenu>,
    pub column_menu: Option<ColumnMenu>,
}
