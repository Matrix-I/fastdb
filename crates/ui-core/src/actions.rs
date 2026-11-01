use gpui::actions;

actions!(
    fastdb,
    [
        Quit,
        About,
        RunQuery,
        CancelQuery,
        SaveChanges,
        DiscardChanges,
        NewTab,
        CloseTab,
        NextTab,
        PrevTab,
        FocusEditor,
        FocusGrid,
        CommandPalette,
        ToggleSidebar,
        ExportCsv,
        ExportSql,
        CheckForUpdates,
    ]
);
