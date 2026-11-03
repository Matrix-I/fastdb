# SQL Editing Essentials Delivery Plan (P1-03)

## Overview

Roadmap milestone **P1-03 — SQL editing essentials** requires:
"Comment/uncomment and four-space indent/outdent toolbar actions operate on selected lines/current line with Undo and autosave. Find/Replace supports case-sensitive literal matching, wrapped navigation, selected-match replacement and one-step Replace All. External UTF-8 `.sql` files import as independent autosaved scripts; Save SQL copy writes a snapshot to another destination. Manual acceptance pending. Richer dialect highlighting/diagnostics, snippets/templates, parameter binding and variable prompts remain open. Acceptance: actions modify/execute the intended selection and remain undoable; parameters are typed safely."

Line commands, literal find/replace, and external file copy/import have already been implemented in earlier deliveries.
This delivery completes the remaining scope:
1. **Richer dialect highlighting & syntax diagnostics**: Comprehensive SQL keywords across ANSI, PostgreSQL, MySQL, and SQLite dialects, parameter token highlighting (`:param`, `?`, `$1`), block comments, and real-time syntax diagnostics (unclosed literals, unclosed block comments, unmatched brackets, and parser errors).
2. **Snippets & Templates**: Reusable, categorized SQL templates (Queries, DML, DDL, CTEs, Joins) accessible via a dedicated "Snippets" toolbar modal with search filtering, keyboard navigation, template preview, and undoable insertion at the cursor or over the active selection.
3. **Parameter Binding and Variable Prompts**: Detection of named (`:name`, `@name`) and positional (`?`, `$1`) query parameters, safe typing (String, Integer, Float, Boolean, Date, NULL, Raw) with SQL injection-proof formatting, and an interactive "Bind Parameters" modal with validation, live preview, tab-level value persistence, and seamless execution.

---

## 1. Architecture & Design

### Crate `crates/sql-intel`
- **`src/params.rs`**:
  - `SqlParameter`: Named or positional parameter representation with name, token kind, and span.
  - `extract_parameters(sql: &str) -> Vec<SqlParameter>`: Token-aware parameter scanner that skips string literals (`'...'`), dollar quotes (`$$...$$`), and comments (`--...`, `/*...*/`), while distinguishing PostgreSQL type casts (`::type`) from named parameters (`:param`).
  - `ParamType`: `String`, `Integer`, `Float`, `Boolean`, `Date`, `Null`, `Raw`.
  - `ParamBinding`: Key, `ParamType`, and raw string value.
  - `substitute_parameters(sql: &str, bindings: &[ParamBinding]) -> Result<String, String>`: Safe parameter substitution preventing SQL injection and formatting strings, numbers, booleans, and nulls deterministically.
- **`src/snippets.rs`**:
  - `SqlSnippet`: Id, title, category (Query, DML, DDL, Advanced), description, template SQL text.
  - `default_snippets() -> Vec<SqlSnippet>`: Standard library of essential templates (Select All, Insert, Update, Delete, Create Table, Create View, Create Index, CTE, Join, Alter Table).
- **`src/diagnostics.rs`**:
  - `SqlDiagnostic`: Line, column, message, severity (`Error`, `Warning`).
  - `diagnose_sql(sql: &str) -> Vec<SqlDiagnostic>`: Checks unclosed string literals, unclosed block comments, unmatched parentheses/brackets, and tree-sitter syntax errors.
- **`src/lib.rs`**: Exposes `params`, `snippets`, `diagnostics`.

### Crate `crates/ui-editor`
- **`src/editor_view.rs`**:
  - Expand keyword dictionary with PostgreSQL, MySQL, SQLite, and transaction keywords.
  - Highlight parameter tokens (`:param`, `$1`, `?`) with dedicated parameter color.
  - Support multi-line block comments (`/* ... */`).

### Crate `crates/app`
- **`src/workspace/snippet_modal.rs`**:
  - `SnippetModal`: Filter text, selected index, snippet list, preview panel.
  - Insertion into editor buffer with full Undo support and editor focus restoration.
- **`src/workspace/params_modal.rs`**:
  - `ParameterBindingModal`: Detected parameter list, per-parameter type selector, text input, validation error display, live substituted SQL preview.
  - Tab-level parameter value persistence so repeated runs retain entered values.
  - Execution callback dispatching substituted SQL to the query runner.
- **`src/workspace/script_view.rs`**:
  - Add "Snippets" button to script toolbar.
  - Display syntax diagnostic indicator/banner when active script contains syntax errors.
- **`src/workspace/query.rs` & `keyboard.rs`**:
  - Intercept execution (`run_active_query`, `explain_active_statement`) when unbound parameters exist, displaying the modal first.
  - Route keyboard inputs (`Escape`, `Enter`, `Tab`, arrows) for both modals.

---

## 2. Safety, Validation & Guardrails

1. **Parameter Safety**:
   - String parameters are strictly escaped (single quotes doubled) and enclosed in `'...'`.
   - Numeric parameters (`Integer`, `Float`) are parsed and validated strictly; invalid inputs are flagged with inline errors preventing execution.
   - Boolean parameters format strictly as `TRUE` or `FALSE`.
   - Parameter extraction never treats PostgreSQL `::` casts as parameters.
   - Parameter substitution never mutates tokens inside string literals or comments.
2. **Editor Undo Integrity**:
   - Snippet insertion executes through `buffer.replace_range()` or `buffer.insert_str()`, seamlessly registering onto the buffer's Undo stack.
3. **No Unintended Side Effects**:
   - Parameter prompt only opens if unresolved parameters actually exist in the target statement.
   - Non-parameterized statements run immediately without interruption.
4. **Performance**:
   - Parameter scanning, snippet filtering, and diagnostic checks run on-demand or during explicit events, keeping the GPUI render path lightweight.

---

## 3. Implementation Steps

1. **Phase 1: `sql-intel` parameter engine, snippets, and diagnostics**:
   - Implement `params.rs`, `snippets.rs`, `diagnostics.rs` with comprehensive unit tests.
2. **Phase 2: `ui-editor` dialect and parameter highlighting**:
   - Expand keyword lists, parameter highlighting, and comment handling.
3. **Phase 3: `app` Snippet modal and toolbar integration**:
   - Create `snippet_modal.rs`, wire toolbar button, handle keyboard events, and verify undoable insertion.
4. **Phase 4: `app` Parameter Binding modal and query execution interception**:
   - Create `params_modal.rs`, wire into `query.rs` and `keyboard.rs`, verify parameter typing, live preview, and execution.
5. **Phase 5: Diagnostics integration and Verification**:
   - Expose diagnostics in script view.
   - Run `cargo fmt`, `cargo clippy`, and full workspace tests.
   - Run independent review and address findings.
