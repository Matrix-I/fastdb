# Extract Keyboard Dialog Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: executing-plans-strict. Steps use `- [ ]`.

**Goal** — Make the existing keyboard precedence for object dialogs and search panels testable and keep it in one place.

**Architecture** — Extract the contiguous object-browser through result-search block in `keyboard.rs` into a pure target selector and a local dispatcher. Keep the selector at the block's current entry position. Preserve the rendering stack and the rest of the key handler.

**Tech stack** — Rust 2024, Cargo, GPUI key events and workspace context.

**Branch** — `chore/keyboard-dialog-routing`.

## Confirmed Requirements

- Preserve existing key bindings, focus behavior, and precedence. The requested planning scope covers the modal routing refactor. The source trace defines the baseline, and the scope excludes any shortcut or rendering-order change.
- Keep Object Browser, Definition Search, Object Search, Schema Action, Import, Parameters, Snippets, Script Search, and Result Search in exactly their current relative order. Any pending column operation or open update blocks every target in this group except Script Search.
- Preserve key consumption for Object Browser even when the key does nothing. Escape and the platform/control close shortcut continue to dismiss it. When the selector returns no target, continue to the existing filter-builder and focus logic.

## Global Constraints

Follow repository `AGENTS.md` and `CLAUDE.md`. In particular, “Do not use Computer Use or request Computer Use permissions. The user verifies the GUI manually. Complete automated checks and report manual GUI acceptance as pending.” “Commit or push only when asked. Never force-push `main`.” “Before calling work done: `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and run the app for UI changes.”

## Technical Defaults

- Start implementation when there is a concrete routing failure or an agreed keybinding requirement, per P3-03 in the [architecture plan](2026-11-03-codebase-architecture-improvements.md#phase-3--architecture-and-updater-verification). Confirm the expected behavior for that case before editing. Any change to bindings or precedence belongs in a separately scoped fix or feature.
- Use the current guards and dispatch order as the parity reference. Rendering has a separate order, and the refactor leaves it alone.
- Collapse the two shared blockers into one boolean at the call site. The original block checks their current synchronous values and returns immediately once a handler runs.
- This can land independently of the [result-state plan](2026-11-19-script-result-state.md) and [staged-cell plan](2026-11-19-staged-cell-lookup.md).

## Risks & Rollback

A moved guard could send Escape to the wrong dialog or let a close shortcut reach the tab. Keep the new call between the Session Monitor and Filter Builder branches. Assert all target priorities and blocker cases. Revert the single-file source-and-test change if precedence differs. There are no dependency, persisted format, public API, or platform binding migrations.

## Task 1 · Select and dispatch the existing dialog target

**Files** — Only `crates/app/src/workspace/keyboard.rs`. Add the selector near the top, replace the block at lines 182–247, and extend the tests at lines 725–822. Inspect lines 43–181 and 248–325 for surrounding event precedence, and `crates/app/src/workspace/mod.rs:455–465,616–686` for the real entry point and render order.

**Consumes** — `Workspace::handle_workspace_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>)` and the existing dialog handlers used by its object-browser through result-search branches.

**Produces** — `DialogKeys::target(self, blocked: bool) -> Option<DialogKeyTarget>` and `Workspace::route_dialog_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool`, both private.

**Test level** — Unit tests for priority and blocker state. GUI checks exercise the real key event entry point. No new database, filesystem, or network boundary.

- [ ] Run `cargo test -p fastdb-app workspace::keyboard` on the baseline. Read the original branch order at lines 182–247 against the test expectations below. Confirm `column_pending.is_none()` is the original check even while a save is pending, and Script Search has neither blocker. Keep the original order even if it differs from draw order.
- [ ] Launch the baseline with `cargo run -p fastdb-app`. Have the user check the reachable cases below through the normal workspace window before extracting the routing. Record the actual recipient and effect. For cases the UI cannot reach, compare the original branches with the table and report baseline GUI coverage as pending. Repeat the same reachable cases after the refactor.

| Baseline setup and key | Expected recipient and effect |
| --- | --- |
| Object Browser open, Escape | Object Browser closes. The active tab stays open. |
| Object Browser open, Cmd+W on macOS or Ctrl+W on Linux | Object Browser closes. The active tab stays open. |
| Object Browser open over an editable script, ordinary character | Object Browser consumes the key and stays open. The script text stays unchanged. |
| Script Search and Result Search both open, ordinary character | Script Search handles the key when no earlier branch claims it. |
| Update open or column edit pending, Script Search open | Script Search still receives a key that reaches this block. With Script Search closed, the blocked dialogs in this block receive no key. |
| No dialog, menu, or other input holding the close shortcut, Cmd/Ctrl+W | The existing workspace handler closes the active tab. |
- [ ] Add this test in the existing `tests` module. Run `cargo test -p fastdb-app keyboard_dialog_priority_matches_existing_branch_order` and expect a compile failure naming the missing selector types.

```rust
#[test]
fn keyboard_dialog_priority_matches_existing_branch_order() {
    use super::{DialogKeyTarget::*, DialogKeys};

    let mut keys = DialogKeys {
        object_browser: true,
        definition_search: true,
        object_search: true,
        schema_action: true,
        import: true,
        parameters: true,
        snippets: true,
        script_search: true,
        result_search: true,
    };

    assert_eq!(keys.target(true), Some(ScriptSearch));
    assert_eq!(keys.target(false), Some(ObjectBrowser));
    keys.object_browser = false;
    assert_eq!(keys.target(false), Some(DefinitionSearch));
    keys.definition_search = false;
    assert_eq!(keys.target(false), Some(ObjectSearch));
    keys.object_search = false;
    assert_eq!(keys.target(false), Some(SchemaAction));
    keys.schema_action = false;
    assert_eq!(keys.target(false), Some(Import));
    keys.import = false;
    assert_eq!(keys.target(false), Some(Parameters));
    keys.parameters = false;
    assert_eq!(keys.target(false), Some(Snippets));
    keys.snippets = false;
    assert_eq!(keys.target(false), Some(ScriptSearch));
    keys.script_search = false;
    assert_eq!(keys.target(false), Some(ResultSearch));
    assert_eq!(keys.target(true), None);
    keys.result_search = false;
    assert_eq!(keys.target(false), None);

    let blocked = DialogKeys {
        object_browser: true,
        definition_search: true,
        object_search: true,
        schema_action: true,
        import: true,
        parameters: true,
        snippets: true,
        result_search: true,
        ..keys
    };
    assert_eq!(blocked.target(true), None);
}
```

- [ ] Add the following types and selector before the existing `impl Workspace`.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogKeyTarget {
    ObjectBrowser,
    DefinitionSearch,
    ObjectSearch,
    SchemaAction,
    Import,
    Parameters,
    Snippets,
    ScriptSearch,
    ResultSearch,
}

#[derive(Clone, Copy)]
struct DialogKeys {
    object_browser: bool,
    definition_search: bool,
    object_search: bool,
    schema_action: bool,
    import: bool,
    parameters: bool,
    snippets: bool,
    script_search: bool,
    result_search: bool,
}

impl DialogKeys {
    fn target(self, blocked: bool) -> Option<DialogKeyTarget> {
        use DialogKeyTarget::*;

        [
            (ObjectBrowser, self.object_browser),
            (DefinitionSearch, self.definition_search),
            (ObjectSearch, self.object_search),
            (SchemaAction, self.schema_action),
            (Import, self.import),
            (Parameters, self.parameters),
            (Snippets, self.snippets),
            (ScriptSearch, self.script_search),
            (ResultSearch, self.result_search),
        ]
        .into_iter()
        .find_map(|(target, open)| (open && (!blocked || target == ScriptSearch)).then_some(target))
    }
}
```

- [ ] Replace exactly the identified block in `handle_workspace_key_down` with the following call, keeping the surrounding returns and handlers in place.

```rust
if self.route_dialog_key(&event.keystroke, cx) {
    return;
}
```

- [ ] Add the dispatcher in the same `impl Workspace`. Read the existing update state and any pending column edit from the same source entities.

```rust
fn route_dialog_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool {
    let dialogs = DialogKeys {
        object_browser: self.object_browser.is_some(),
        definition_search: self.definition_search.is_some(),
        object_search: self.object_search.is_some(),
        schema_action: self.schema_action_modal.is_some(),
        import: self.import_modal.is_some(),
        parameters: self.params_modal.is_some(),
        snippets: self.snippet_modal.is_some(),
        script_search: self.script_search.is_some(),
        result_search: self.result_search.is_some(),
    };
    let blocked = self.column_pending.is_some() || self.updates.read(cx).is_open();
    let Some(target) = dialogs.target(blocked) else {
        return false;
    };
    match target {
        DialogKeyTarget::ObjectBrowser => {
            if key.key == "escape"
                || ((key.modifiers.platform || key.modifiers.control)
                    && key.key.eq_ignore_ascii_case("w"))
            {
                self.object_browser = None;
                cx.notify();
            }
        }
        DialogKeyTarget::DefinitionSearch => self.definition_search_key(key, cx),
        DialogKeyTarget::ObjectSearch => self.object_search_key(key, cx),
        DialogKeyTarget::SchemaAction => self.schema_action_modal_key(key, cx),
        DialogKeyTarget::Import => self.import_modal_key(key, cx),
        DialogKeyTarget::Parameters => self.handle_params_modal_key(key, cx),
        DialogKeyTarget::Snippets => self.handle_snippet_modal_key(key, cx),
        DialogKeyTarget::ScriptSearch => self.search_key(key, cx),
        DialogKeyTarget::ResultSearch => self.result_search_key(key, cx),
    }
    true
}
```

- [ ] Run `cargo test -p fastdb-app keyboard_dialog_priority_matches_existing_branch_order` and `cargo test -p fastdb-app workspace::keyboard`. Expect the new precedence tests and the existing Escape, Tab, record-view, and navigator tests to pass. Ready to commit `crates/app/src/workspace/keyboard.rs` as `refactor(app): extract keyboard dialog routing` after verification and explicit authorization.

## Task 2 · Verification & Docs

**Files** — Review [shortcuts](../guide/shortcuts.md) and the related SQL editor help. The refactor preserves documented behavior and needs no guide, README, or provider-doc changes.

**Consumes** — Task 1. **Produces** — full verification results, parity evidence, and independent review verdict. **Test levels** — workspace unit/integration, desktop end-to-end.

- [ ] Run `cargo fmt --all` then inspect `git status --short`, the full `git diff HEAD`, and any relevant untracked files. Run `git diff --check` and check for unrelated changes, debug output, or secrets.
- [ ] Run `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --all-targets`, and `cargo test --workspace`. Record each outcome and provider tests skipped.
- [ ] Run `cargo run -p fastdb-app`. User desktop acceptance checks Escape and Cmd/Ctrl+W in Object Browser, typing in Script Search and Result Search, and unchanged tab close when no dialog handles the key. Where normal UI actions permit overlapping dialogs, compare observed priority with the baseline and test that Script Search keeps its exception for update/column guards. Record which cases and platforms were exercised. The deterministic unit test covers combinations that cannot be opened through desktop controls.
- [ ] Self-review target order, both blockers, consumption of non-closing keys, fallthrough, unchanged surrounding handlers and root wiring, and the absence of added allocations, I/O, or APIs. Apply `code-review-strict` to the final diff, address findings or report them, and repeat verification after edits. Clean up temporary verification artifacts and keep the runnable binary and reusable dependencies.
