# Navigator search filter

## Goal

Replace the separate **Search objects…** and **Browse catalog objects…** controls with one navigator search field and a filter-mode icon. Clicking the icon opens four modes; hovering it explains the active mode. Search results are limited to the selected mode.

## Assumptions

- **Files** means SQL script files saved by FastDB.
- Connection and schema/relation filtering uses currently loaded workspace metadata and does not issue database queries.
- Selecting a connection result activates/connects to it; selecting a relation opens its table tab; selecting a saved script opens that file.
- The existing catalog browser remains reachable from the search control when the relation/catalog mode is selected with an empty query.

## Implementation steps

1. Add a small navigator search model with the four modes, query-edit behavior, case-insensitive matching, and unit tests for each mode's matching semantics.
2. Replace the two navigator actions with one line-input field, a filter icon, a four-option menu, active-mode tooltip text, and a bounded result list.
3. Wire results to existing connection activation, relation-tab, saved-script, and catalog-browser flows; keep Reveal active object available.
4. Update the navigator guide and verify formatting, workspace lint/tests, app build, and startup.

## Acceptance

- Exactly one search field is shown in the navigator.
- Clicking the filter icon shows Connections, Catalogs and schemas, Tables/procedures/etc., and Files.
- Hover text names the active filter mode.
- Changing modes preserves the query and changes only the result kind; clearing the query restores the normal navigator tree.
- Result rows use existing navigation/open flows and do not silently lose catalog browsing.
