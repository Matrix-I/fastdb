# Share Staged Cell Lookup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: executing-plans-strict. Steps use `- [ ]`.

**Goal** — Give the Value Panel, Value Editor, and Record view one borrowed interpretation of a staged cell.

**Architecture** — Add a private app workspace module that reads `Changeset` for an already resolved `RowRef`. Consumers keep their existing pager reads, error handling, formatting, and preview limits.

**Tech stack** — Rust 2024, Cargo, GPUI, `ui_grid::Changeset`.

**Branch** — `chore/staged-cell-lookup`.

## Confirmed Requirements

- Share lookup for these three surfaces. Preserve the distinctions among unchanged fetched cells, staged SQL NULL, text including an empty string, and unset database defaults.
- Preserve which cells can be edited, single-step undo, stale preview/editor guards, record paging, binary interpretation, and each view's byte limits. Preserve write paths and storage formats.

## Global Constraints

Follow repository `AGENTS.md` and `CLAUDE.md`. In particular, “Keep UI work off the render path: GPUI re-renders on every key press or hover, so avoid heavy work in `render`.” “Commit or push only when asked. Never force-push `main`.” “Do not use Computer Use or request Computer Use permissions. The user verifies the GUI manually. Complete automated checks and report manual GUI acceptance as pending.”

## Technical Defaults

- A workspace-private helper avoids expanding the `ui-grid` public API. Borrow text and resolve each displayed row once. Continue to allocate owned strings only at the existing worker boundary after the size check.
- Inserted row writers maintain one value per column, and setting text clears the explicit NULL bit. For an inserted row, honor an explicit NULL marker before looking up its value slot, as Record does today. If the row is missing or the value slot is missing without an explicit marker, return `None`. Value Panel and Value Editor skip that invalid state, and Record uses its existing default fallback.
- This can land independently of the [result-state plan](2026-11-19-script-result-state.md). If that plan lands first, use `tab.current_result.grid` wherever the current code uses `tab.grid`.

## Risks & Rollback

A merged lookup could collapse NULL and default, or let a worker use a stale snapshot. Keep the four explicit cases, existing identity/revision keys, and error/size checks. Revert the helper and its three callers together if parity fails. No persistence, external API, provider, format, or dependency migration is needed.

## Task 1 · Centralize borrowed lookup

**Files** — Add `crates/app/src/workspace/staged_cell.rs` and declare `mod staged_cell;` in `crates/app/src/workspace/mod.rs` near the module declarations at lines 77–103. Migrate `record_view.rs:81–122`, `value_panel.rs:179–257`, and `value_editor.rs:211–238` under that directory.

**Consumes** — `Changeset::resolve_row(u64) -> RowRef`, `Changeset::edited_value(u64, usize) -> Option<Option<&str>>`, and `Changeset::inserted_rows`.

**Produces** — `StagedCell::lookup(&Changeset, RowRef, usize) -> Option<StagedCell<'_>>` and `StagedCell::value(self) -> Option<Option<&str>>` with workspace-only visibility.

**Test level** — Unit tests with a real `Changeset`. Existing app tests exercise real pagers and guard logic. This change adds no database, filesystem, or network calls.

- [ ] Run the existing focused suites before editing to characterize the baseline.

```bash
cargo test -p fastdb-app workspace::record_view
cargo test -p fastdb-app workspace::value_panel
cargo test -p fastdb-app workspace::value_editor
```

- [ ] Declare `mod staged_cell;` and start the file with the following test module. Run `cargo test -p fastdb-app staged_lookup_preserves_cell_states_and_row_positions`. Expect compilation to fail because `StagedCell` is absent.

```rust
#[cfg(test)]
mod tests {
    use super::StagedCell::{self, Default, Null, Text, Unchanged};
    use ui_grid::{Changeset, RowRef};

    #[test]
    fn staged_lookup_preserves_cell_states_and_row_positions() {
        let mut changes = Changeset::new();
        changes.insert_row_at(1, vec![None, None, Some(String::new()), Some("text".into())]);
        changes.inserted_rows[0].explicit_nulls.insert(1);
        changes.set_cell(1, 0, Some(b"old".to_vec()), None);
        changes.set_cell(1, 1, Some(b"old".to_vec()), Some("new".into()));

        let inserted = changes.resolve_row(1);
        assert_eq!(inserted, RowRef::New(0));
        for (col, expected) in [Default, Null, Text(""), Text("text")].into_iter().enumerate() {
            let actual = StagedCell::lookup(&changes, inserted, col).unwrap();
            assert_eq!(actual, expected);
        }
        assert_eq!(StagedCell::lookup(&changes, inserted, 4), None);
        assert_eq!(StagedCell::lookup(&changes, RowRef::New(1), 0), None);

        let fetched = changes.resolve_row(2);
        assert_eq!(fetched, RowRef::Fetched(1));
        for (col, expected) in [Null, Text("new"), Unchanged].into_iter().enumerate() {
            assert_eq!(StagedCell::lookup(&changes, fetched, col), Some(expected));
        }

        assert_eq!(Unchanged.value(), None);
        assert_eq!(Null.value(), Some(None));
        assert_eq!(Default.value(), Some(None));
        assert_eq!(Text("").value(), Some(Some("")));
    }
}
```

- [ ] Implement the helper above the tests.

```rust
use ui_grid::{Changeset, RowRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StagedCell<'a> {
    Unchanged,
    Default,
    Null,
    Text(&'a str),
}

impl<'a> StagedCell<'a> {
    pub(super) fn lookup(changes: &'a Changeset, row: RowRef, col: usize) -> Option<Self> {
        match row {
            RowRef::Fetched(index) => Some(match changes.edited_value(index, col) {
                None => Self::Unchanged,
                Some(None) => Self::Null,
                Some(Some(text)) => Self::Text(text),
            }),
            RowRef::New(index) => {
                let row = changes.inserted_rows.get(index)?;
                if row.explicit_nulls.contains(&col) {
                    return Some(Self::Null);
                }
                Some(row.values.get(col)?.as_deref().map_or(Self::Default, Self::Text))
            }
        }
    }

    pub(super) fn value(self) -> Option<Option<&'a str>> {
        match self {
            Self::Unchanged => None,
            Self::Default | Self::Null => Some(None),
            Self::Text(text) => Some(Some(text)),
        }
    }
}
```

- [ ] Import `super::staged_cell::StagedCell` at each caller. In `value_panel.rs` replace the top-level `use ui_grid::RowRef;` with that import and retain the test module's separate `RowRef` import. After the row-count guard and before `let key`, use the following code. Set `PreviewKey::default` to `default`; remove the second row lookup and its staged-value match after creating `PreviewJob`.

```rust
let row = grid.changeset.resolve_row(focus.row);
let cell = StagedCell::lookup(&grid.changeset, row, focus.col)?;
let default = cell == StagedCell::Default;
```

- [ ] Immediately after `let job = PreviewJob(self.value_panel.job.clone());` bind `let staged = cell.value();`. Leave the existing size check before the owned clone and keep the async completion test against the same preview key.
- [ ] In `value_editor.rs`, keep `RowRef` and the fetched-row deletion guard. Replace the `let (staged, default) = match row` block with the following code. Retain the staged-value size checks, `stage_value`, and source identity checks.

```rust
let Some(cell) = StagedCell::lookup(&tab.grid.changeset, row, focus.col) else {
    return;
};
let default = cell == StagedCell::Default;
let staged = cell.value();
```

- [ ] In `record_view.rs`, replace `project_row` with the code below. Retain `preview`, `read_record`, and the column range logic.

```rust
fn project_row(changes: &Changeset, row: u64, columns: std::ops::Range<usize>) -> RowProjection {
    let row = changes.resolve_row(row);
    let (fetched, state) = match row {
        RowRef::Fetched(index) if changes.is_deleted(index) => (Some(index), "Marked for deletion"),
        RowRef::Fetched(index) => (Some(index), "Loaded row"),
        RowRef::New(_) => (None, "New row"),
    };
    let values = columns
        .map(|col| match StagedCell::lookup(changes, row, col) {
            Some(StagedCell::Unchanged) => None,
            Some(StagedCell::Null) => Some(Field::Null),
            Some(StagedCell::Text(text)) => Some(preview(text.as_bytes())),
            Some(StagedCell::Default) | None => Some(Field::Default),
        })
        .collect();
    RowProjection { fetched, state, values }
}
```

- [ ] Run all four focused test commands above with the implementation in place. Expect the new test to pass and the three existing suites to stay green. Ready to commit the five workspace files as `refactor(app): share staged cell lookup` after verification and explicit authorization.

## Task 2 · Verification & Docs

**Files** — Review [type handling](../guide/type-handling.md) and [navigator and tables](../guide/navigator-and-tables.md). Preserve their documented behavior. The implementation needs no guide, README, or provider-doc changes.

**Consumes** — Task 1. **Produces** — full verification results and review verdict. **Test levels** — workspace unit/integration, desktop end-to-end.

- [ ] Run `cargo fmt --all` then inspect `git status --short`, the full `git diff HEAD`, and all untracked implementation files. Run `git diff --check` and check for unrelated changes, debug output, or secrets.
- [ ] Run `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --all-targets`, and `cargo test --workspace`. Record each outcome and provider tests skipped.
- [ ] Run `cargo run -p fastdb-app`. User desktop acceptance on a disposable editable table with nullable text verifies new-row default, explicit NULL, and empty text in Panels, Value Editor, and Record. Also stage an edit to a fetched cell, inspect it across those surfaces, and undo it. Check that switching selection during a pending preview cannot display stale data for the new cell. Leave staged changes unsaved and close using Cancel. Record launch failures and platforms not exercised.
- [ ] Self-review the four cases, malformed-index fallback, borrowed data before the worker clone, bounds/size checks, stale async guards, names and stable API use, write-path isolation, and tests. Apply `code-review-strict` to the final diff, address findings or report them, and repeat verification after edits. Clean up temporary verification artifacts and keep the runnable binary and reusable dependencies.
