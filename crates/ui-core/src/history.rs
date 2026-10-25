//! Snapshot-based undo/redo shared by every editor in the app: text inputs, the SQL
//! editor and the staged edits of the result grid.

/// Undo steps kept per history before the oldest ones are dropped
const MAX_STEPS: usize = 500;

/// What an edit did, so runs of the same kind of edit collapse into one undo step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Typing characters other than whitespace
    Typing,
    /// Deleting one character at a time
    Deleting,
    /// Anything else (paste, cut, whitespace, a staged grid change): always its own step
    Other,
}

/// Undo and redo stacks of snapshots of some editable state `T`.
///
/// Call [`History::record`] with the state *before* each change; [`History::undo`] and
/// [`History::redo`] take the current state and hand back the one to restore.
#[derive(Debug, Clone)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    /// Kind of the last recorded edit while it can still absorb the next one
    open: Option<EditKind>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), open: None }
    }
}

impl<T> History<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Saves `before` as an undo step, unless the edit continues the run of the last one
    pub fn record(&mut self, before: T, kind: EditKind) {
        self.redo.clear();
        let continues = kind != EditKind::Other && self.open == Some(kind);
        self.open = (kind != EditKind::Other).then_some(kind);
        if continues {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
    }

    /// Ends the current run, so the next edit starts a new undo step (caret moved, focus changed)
    pub fn break_run(&mut self) {
        self.open = None;
    }

    /// The state to restore for undo, or `None` when there is nothing to undo
    pub fn undo(&mut self, current: T) -> Option<T> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.open = None;
        Some(previous)
    }

    /// The state to restore for redo, or `None` when there is nothing to redo
    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.open = None;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.open = None;
    }
}

/// Whether a key press is undo (Cmd/Ctrl+Z) or redo (Cmd/Ctrl+Shift+Z, Cmd/Ctrl+Y)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoKey {
    Undo,
    Redo,
}

impl UndoKey {
    pub fn from_keystroke(keystroke: &gpui::Keystroke) -> Option<Self> {
        let mods = &keystroke.modifiers;
        if !(mods.platform || mods.control) {
            return None;
        }
        match keystroke.key.to_ascii_lowercase().as_str() {
            "z" if mods.shift => Some(Self::Redo),
            "z" => Some(Self::Undo),
            "y" => Some(Self::Redo),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_runs_collapse_into_one_step() {
        let mut h = History::new();
        h.record("", EditKind::Typing);
        h.record("a", EditKind::Typing);
        h.record("ab", EditKind::Other);
        h.record("ab ", EditKind::Typing);
        assert_eq!(h.undo("ab c"), Some("ab "));
        assert_eq!(h.undo("ab "), Some("ab"));
        assert_eq!(h.undo("ab"), Some(""));
        assert_eq!(h.undo(""), None);
        assert_eq!(h.redo(""), Some("ab"));
    }

    #[test]
    fn a_new_edit_drops_redo() {
        let mut h = History::new();
        h.record(1, EditKind::Other);
        assert_eq!(h.undo(2), Some(1));
        h.record(1, EditKind::Other);
        assert_eq!(h.redo(3), None);
    }

    #[test]
    fn break_run_starts_a_new_step() {
        let mut h = History::new();
        h.record("", EditKind::Typing);
        h.break_run();
        h.record("a", EditKind::Typing);
        assert_eq!(h.undo("ab"), Some("a"));
    }
}
