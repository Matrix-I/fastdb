use gpui::actions;

actions!(
    fastdb,
    [
        Quit,
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
    ]
);
