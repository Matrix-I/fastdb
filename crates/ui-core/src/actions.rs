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
        ToggleTheme,
        ZoomIn,
        ZoomOut,
        ResetZoom,
        ExportCsv,
        ExportSql,
        CheckForUpdates,
    ]
);
