//! Column name suggestions for the filter bar: typing part of a column name opens a
//! list under the input, Up/Down pick an entry and Enter, Tab or a click replaces the
//! typed word with the quoted column name. Ctrl+Space opens the list for an empty word.

use std::ops::Range;

use gpui::prelude::FluentBuilder;
use mem_engine::DataType;
use ui_core::history::EditKind;

use super::query::quote_ident;

use super::*;

/// Most entries shown at once; Up/Down scroll the list past them
const VISIBLE_ITEMS: usize = 10;

/// Open suggestion list of the filter bar
#[derive(Debug, Clone)]
pub struct FilterSuggest {
    pub items: Vec<(String, DataType)>,
    pub selected: usize,
    /// Fits the longest column name of the table, so the list keeps its width while typing
    pub width: f32,
}

/// Char range of the column name being typed at `caret`, with its opening quote if any,
/// and the typed part before the caret. `None` inside a string literal.
fn word_at(text: &str, caret: usize) -> Option<(Range<usize>, String)> {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    if chars[..caret].iter().filter(|&&c| c == '\'').count() % 2 == 1 {
        return None;
    }
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = caret;
    while start > 0 && is_ident(chars[start - 1]) {
        start -= 1;
    }
    let prefix: String = chars[start..caret].iter().collect();
    let mut end = caret;
    while end < chars.len() && is_ident(chars[end]) {
        end += 1;
    }
    // An already opened quote is replaced together with the word, and so is its closing one
    if start > 0 && matches!(chars[start - 1], '"' | '`') {
        let quote = chars[start - 1];
        start -= 1;
        if chars.get(end) == Some(&quote) {
            end += 1;
        }
    }
    Some((start..end, prefix))
}

/// Columns matching `prefix`: names starting with it first, then names containing it.
/// Empty when the only match is the word itself, so a finished name closes the list.
fn matching_columns(columns: &[(String, DataType)], prefix: &str, allow_empty: bool) -> Vec<(String, DataType)> {
    if prefix.is_empty() && !allow_empty {
        return Vec::new();
    }
    let needle = prefix.to_lowercase();
    let (mut starts, mut contains): (Vec<_>, Vec<_>) = (Vec::new(), Vec::new());
    for col in columns {
        let name = col.0.to_lowercase();
        if name.starts_with(&needle) {
            starts.push(col.clone());
        } else if name.contains(&needle) {
            contains.push(col.clone());
        }
    }
    starts.extend(contains);
    if starts.len() == 1 && starts[0].0 == prefix {
        starts.clear();
    }
    starts
}

impl Workspace {
    /// Columns of the active tab: those of the loaded result, else the table's metadata
    fn filter_columns(&self) -> Vec<(String, DataType)> {
        let Some(tab) = self.tabs.get(self.active_tab) else { return Vec::new() };
        if let Some(pager) = tab.grid.pager.as_ref() {
            return pager.columns.iter().map(|c| (c.name.clone(), c.data_type)).collect();
        }
        let key = (tab.schema_name.clone().unwrap_or_default(), tab.table_name.clone().unwrap_or_default());
        self.table_columns
            .get(&key)
            .map(|cols| cols.iter().map(|c| (c.name.clone(), c.data_type)).collect())
            .unwrap_or_default()
    }

    /// Reopens or closes the list for the word at the filter caret. `force` (Ctrl+Space)
    /// lists every column even before anything is typed.
    pub(super) fn refresh_filter_suggest(&mut self, force: bool) {
        let Some(tab) = self.tabs.get(self.active_tab) else { return };
        let has_selection = selection_range(self.filter_anchor, self.filter_cursor, tab.filter_sql.chars().count()).is_some();
        let columns = self.filter_columns();
        let items = match word_at(&tab.filter_sql, self.filter_cursor) {
            Some((_, prefix)) if !has_selection => matching_columns(&columns, &prefix, force),
            _ => Vec::new(),
        };
        let longest = columns
            .iter()
            .map(|(name, _)| ui_grid::VirtualDataGrid::estimate_text_width(name, 12.0))
            .fold(0.0, f32::max);
        // Keep the highlighted entry while it still matches
        let selected = self
            .filter_suggest
            .as_ref()
            .and_then(|s| s.items.get(s.selected))
            .and_then(|(name, _)| items.iter().position(|(n, _)| n == name))
            .unwrap_or(0);
        let width = (longest + 64.0).clamp(240.0, 480.0);
        self.filter_suggest = (!items.is_empty()).then_some(FilterSuggest { items, selected, width });
    }

    /// Keys for the open list; returns whether the key was used
    pub(super) fn handle_filter_suggest_key(&mut self, keystroke: &Keystroke) -> bool {
        let Some(suggest) = self.filter_suggest.as_mut() else { return false };
        let len = suggest.items.len();
        match keystroke.key.as_str() {
            "down" => suggest.selected = (suggest.selected + 1) % len,
            "up" => suggest.selected = (suggest.selected + len - 1) % len,
            "escape" => self.filter_suggest = None,
            "enter" | "return" | "tab" => {
                let index = suggest.selected;
                self.apply_filter_suggest(index);
            }
            _ => return false,
        }
        true
    }

    /// Replaces the word at the caret with the quoted name of entry `index`
    pub(super) fn apply_filter_suggest(&mut self, index: usize) {
        let Some(suggest) = self.filter_suggest.take() else { return };
        let Some((name, _)) = suggest.items.get(index) else { return };
        let quoted = quote_ident(self.selected_driver_kind, name);
        let Some(tab) = self.tabs.get_mut(self.active_tab) else { return };
        let Some((range, _)) = word_at(&tab.filter_sql, self.filter_cursor) else { return };

        let len = tab.filter_sql.chars().count();
        tab.filter_history.record((tab.filter_sql.clone(), self.filter_cursor.min(len)), EditKind::Other);
        let byte = |i: usize| tab.filter_sql.char_indices().nth(i).map(|(b, _)| b).unwrap_or(tab.filter_sql.len());
        let (start, end) = (byte(range.start), byte(range.end));
        tab.filter_sql.replace_range(start..end, &quoted);
        self.filter_cursor = range.start + quoted.chars().count();
        self.filter_anchor = None;
        self.cursor_visible = true;
    }

    /// The list under the filter input, if open
    pub(super) fn render_filter_suggest(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let suggest = self.filter_suggest.as_ref().filter(|_| self.is_filter_focused)?;
        let theme = &self.theme;
        let first = suggest.selected.saturating_sub(VISIBLE_ITEMS - 1);

        let panel = div()
            .id("filter-suggest")
            .occlude()
            .flex()
            .flex_col()
            .w(px(suggest.width))
            .p_1()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.filter_suggest = None;
                cx.notify();
            }))
            .children(suggest.items.iter().enumerate().skip(first).take(VISIBLE_ITEMS).map(|(idx, (name, data_type))| {
                let is_sel = idx == suggest.selected;
                div()
                    .id(("filter-suggest-item", idx))
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(22.0))
                    .px_2()
                    .rounded_sm()
                    .text_size(px(12.0))
                    .text_color(theme.text_primary)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .when(is_sel, |d| d.bg(theme.bg_surface_selected))
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply_filter_suggest(idx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(22.0))
                            .text_size(px(9.5))
                            .text_color(rgb(0x58a6ff))
                            .child(ui_grid::dbeaver_type_badge(*data_type)),
                    )
                    .child(name.clone())
            }));

        Some(
            div()
                .absolute()
                .top(px(24.0))
                .left_0()
                .child(deferred(anchored().snap_to_window().child(panel)).with_priority(1)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{matching_columns, word_at};
    use mem_engine::DataType;

    fn cols(names: &[&str]) -> Vec<(String, DataType)> {
        names.iter().map(|n| (n.to_string(), DataType::Text)).collect()
    }

    fn names(items: Vec<(String, DataType)>) -> Vec<String> {
        items.into_iter().map(|(n, _)| n).collect()
    }

    #[test]
    fn finds_the_word_at_the_caret() {
        assert_eq!(word_at("sta", 3), Some((0..3, "sta".to_string())));
        assert_eq!(word_at("id = 1 AND sta", 14), Some((11..14, "sta".to_string())));
        // The rest of the word after the caret is replaced too
        assert_eq!(word_at("status = 1", 3), Some((0..6, "sta".to_string())));
        // An opened quote, and its closing one, are part of the word
        assert_eq!(word_at("\"sta", 4), Some((0..4, "sta".to_string())));
        assert_eq!(word_at("\"sta\" = 1", 4), Some((0..5, "sta".to_string())));
        assert_eq!(word_at("name = 'sta", 11), None);
    }

    #[test]
    fn prefix_matches_come_before_substring_matches() {
        let columns = cols(&["id", "statusReason", "lastStatus", "statusDetails"]);
        assert_eq!(names(matching_columns(&columns, "sta", false)), ["statusReason", "statusDetails", "lastStatus"]);
        assert_eq!(names(matching_columns(&columns, "STA", false)).len(), 3);
        assert!(matching_columns(&columns, "", false).is_empty());
        assert_eq!(matching_columns(&columns, "", true).len(), 4);
        // A fully typed name closes the list
        assert!(matching_columns(&columns, "id", false).is_empty());
    }
}
