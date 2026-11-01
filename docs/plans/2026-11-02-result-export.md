# Result export from the UI

Expose the existing streaming CSV and SQL INSERT exporters in the shared result
footer for table and script tabs. Export the fetched result rows, not unsaved grid
edits or rows that have not been fetched. Label this scope in the UI.

1. Add export buttons and per-tab status, with native save dialogs. Capture the
   originating tab and pager before awaiting the dialog. Perform file writes on a
   background executor and publish files atomically to avoid partial output.
2. Support schema-qualified SQL targets and provider-specific identifier/literal
   quoting using the existing SQL dialect helpers. Script results target
   `query_result`. Keep the existing SQL export entry point compatible.
3. Cover serialization, batching, file failures and result export eligibility with
   tests. Update README with the behavior and limitations.
4. Run formatting, workspace clippy and tests, then launch the app for a UI smoke
   test.

