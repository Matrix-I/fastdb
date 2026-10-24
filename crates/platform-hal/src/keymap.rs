#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsModifier {
    Command,
    Control,
}

pub fn primary_modifier() -> OsModifier {
    #[cfg(target_os = "macos")]
    {
        OsModifier::Command
    }
    #[cfg(not(target_os = "macos"))]
    {
        OsModifier::Control
    }
}

pub fn primary_modifier_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "ctrl"
    }
}

pub fn primary_modifier_symbol() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "⌘"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl+"
    }
}

pub fn shortcut_display(key: &str) -> String {
    format!("{}{}", primary_modifier_symbol(), key)
}

pub const ACTION_RUN_QUERY: &str = "run_query";
pub const ACTION_SAVE_CHANGES: &str = "save_changes";
pub const ACTION_NEW_TAB: &str = "new_tab";
pub const ACTION_CLOSE_TAB: &str = "close_tab";
pub const ACTION_COMMAND_PALETTE: &str = "command_palette";
