# Schema and Object Editing Delivery Plan (P1-02)

## Overview

Roadmap milestone **P1-02 — Schema/object editing** requires:
"Create/alter/rename/drop tables, columns, constraints, indexes and views; truncate with confirmation.
Acceptance: SQL preview → execute → persisted catalog change → navigator refresh, including rollback or clear partial-failure reporting where DDL is nontransactional."

This document establishes the architecture, lifecycle contracts, dialect handling, and automated verification plan for schema and object editing.

---

## 1. Scope & Action Matrix

The editing workflows must be accessible from the Database Navigator and Properties panels, following the verified pattern established in column editing (P0-07):

| Target Object | Action | Required Inputs / Confirmation | DDL Generated |
| :--- | :--- | :--- | :--- |
| **Table** | **Truncate** | Warning confirmation dialog | `TRUNCATE TABLE "schema"."table" [CASCADE]` |
| **Table** | **Drop** | Confirmation dialog with CASCADE checkbox | `DROP TABLE "schema"."table" [CASCADE / RESTRICT]` |
| **Table** | **Rename** | New table name input | `ALTER TABLE "schema"."table" RENAME TO "new_name"` |
| **Table** | **Create** | Schema, name, column list (or SQL template) | `CREATE TABLE "schema"."table" (...)` |
| **View** | **Drop** | Confirmation dialog with CASCADE checkbox | `DROP VIEW "schema"."view" [CASCADE]` |
| **View** | **Rename** | New view name input | `ALTER VIEW "schema"."view" RENAME TO "new_name"` |
| **View** | **Create** | Schema, name, query definition | `CREATE VIEW "schema"."view" AS ...` |
| **Column** | **Add** | Column name, type, nullability, default | `ALTER TABLE "schema"."table" ADD COLUMN "col" ...` |
| **Column** | **Drop** | Confirmation dialog with CASCADE option | `ALTER TABLE "schema"."table" DROP COLUMN "col" [CASCADE]` |
| **Column** | **Rename** | New column name | `ALTER TABLE "schema"."table" RENAME COLUMN "old" TO "new"` |
| **Constraint** | **Drop** | Confirmation dialog | `ALTER TABLE "schema"."table" DROP CONSTRAINT "name"` |
| **Index** | **Drop** | Confirmation dialog | `DROP INDEX "schema"."index"` |

*(Note: Column attribute alterations—type, nullability, default, comment—are already handled by the column editor in P0-07.)*

---

## 2. Interaction & Lifecycle Contract

Every destructive or structural modification must adhere to the 5-step lifecycle:
```
1. Trigger (Navigator right-click context menu or Properties toolbar)
   ↓
2. Parameter / Confirmation dialog (Prompt for inputs, safety warnings)
   ↓
3. SQL Preview & Review (Syntax-highlighted/formatted SQL DDL before execution)
   ↓
4. Execution on dedicated driver connection with timeout and isolation
   ↓
5. Feedback & Catalog Refresh (Display success/error; on success refresh affected schema/navigator nodes)
```

### Safety & Guardrails:
1. **Confirmation before Destruction**: `DROP` and `TRUNCATE` actions display explicit danger banners requiring user confirmation.
2. **SQL Preview**: Users must be able to inspect and copy the generated SQL statements before executing.
3. **Transactionality**:
   - PostgreSQL and SQLite support transactional DDL (`BEGIN ... DDL ... COMMIT`). In case of failure, changes are rolled back automatically.
   - MySQL executes DDL with implicit commits. The dialog must explicitly inform the user when an operation is non-transactional.
4. **Active Connection & Generation Guard**: If the target connection disconnects or reconnects before execution, in-flight DDL prompts must fail closed with a `"Connection state changed. Re-open action."` notice.
5. **Navigator Synchronization**: Upon successful execution, the navigator automatically refreshes the target schema's relations and closes the modal dialog.

---

## 3. Architecture & Module Design

### Crate `crates/db-wire` (Driver & DDL Generation)
- **`crates/db-wire/src/sql/schema_ddl.rs`**:
  - Strongly-typed DDL action definitions:
    - `TableAction::Truncate { cascade: bool }`
    - `TableAction::Drop { cascade: bool }`
    - `TableAction::Rename { new_name: String }`
    - `ColumnAction::Add(ColumnDef)`
    - `ColumnAction::Drop { name: String, cascade: bool }`
    - `ColumnAction::Rename { old_name: String, new_name: String }`
    - `IndexAction::Drop { schema: String, name: String }`
    - `ConstraintAction::Drop { name: String }`
  - Dialect-aware SQL generator:
    - PostgreSQL: double-quoted identifiers, `CASCADE` support, `ALTER TABLE ... RENAME TO`.
    - SQLite: identifier quoting, limited alter support (rename table, rename column, add column).
    - MySQL: backtick quoting, `RENAME TABLE`, constraint dropping differences.
- **`crates/db-wire/src/traits.rs`**:
  - Add `async fn execute_ddl(&self, sql: &str) -> Result<()>` (or reuse `execute_query` / dedicated connection).

### Crate `crates/app` (UI & Orchestration)
- **`crates/app/src/workspace/schema_editor/`**:
  - `schema_action_modal.rs`: Modal UI for action input, confirmation, SQL preview, and Execute/Cancel controls.
  - `table_actions.rs`: Action state management and execution dispatch.
- **`crates/app/src/workspace/nav_context_menu.rs` & `navigator.rs`**:
  - Extend navigator context menus to support right-clicking on:
    - Tables: Truncate, Drop, Rename, View Properties, Open Query.
    - Views: Drop, Rename, View Properties.
    - Folders: Create Table, Create View.

---

## 4. Phased Implementation Plan

1. **Phase 1: DDL Generation & Verification in `db-wire`**:
   - Implement `crates/db-wire/src/sql/schema_ddl.rs` with comprehensive tests for PostgreSQL, SQLite, and MySQL dialect variants.
   - Add unit tests verifying identifier escaping (special characters, SQL injection attempts, case-preservation).

2. **Phase 2: DDL Execution Trait in `db-wire`**:
   - Wire driver execution on dedicated connection with timeouts.
   - Integration tests against live PostgreSQL and SQLite for table rename, drop, truncate, add column, drop column, and index drop.

3. **Phase 3: Schema Action Modal in `fastdb-app`**:
   - Implement modal dialog with tabs/steps: Input/Confirm → Preview SQL → Execute.
   - Wire keyboard shortcuts (`Escape`, `Enter`).

4. **Phase 4: Navigator & Properties Context Menus**:
   - Wire right-click menus on tables, views, columns in `crates/app/src/workspace/navigator/`.
   - On execution completion, trigger navigator schema reload.

5. **Phase 5: Self-Review & Automated Workspace Verification**:
   - `cargo fmt --all --check`
   - `cargo clippy --workspace --all-targets --locked`
   - `cargo test --workspace --locked`
   - Independent subagent code review.
