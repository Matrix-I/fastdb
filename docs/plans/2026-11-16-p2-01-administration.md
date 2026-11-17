# P2-01 PostgreSQL Administration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Complete P2-01's PostgreSQL administration workflow using provider-neutral driver APIs, native catalog queries, and a connection-bound GPUI SQL dialog. Rust, GPUI, tokio-postgres. Branch `feature/p2-01-administration`; base `e4ea1fe`.

## Confirmed Requirements

- User requested “P2-01 Administration implement it” and set the objective “complete P2-01 Administration”.
- On 2026-11-16 the user approved PostgreSQL catalog browsing plus editable SQL templates with preview/confirmation, rather than dedicated form editors.
- The user approved additive `DatabaseDriver` methods `supports_administration()`, `administration_catalog(family, after)`, and `execute_administration(sql, cancel)` and the `AdminFamily`, `AdminObject`, `AdminPage`, `AdminOutcome` types. Other providers return explicit unsupported errors.
- The user approved existing Linux build prerequisites and PostgreSQL tooling. Sudo requires interactive authentication; extract approved packages under `/tmp` instead.
- Manual desktop acceptance belongs to the user. Do not use Computer Use. Commit or push only when asked.

## Global Constraints

- “Drivers stream rows into mem-engine as batches arrive; never collect a full result set in memory.” Administration retains bounded previews and streams/discards excess rows; normal result streaming remains unchanged.
- “Keep UI work off the render path”; SQL, native catalog work and filesystem I/O run on the background executor. Render cached data.
- “New database support goes through the db-wire traits”; PostgreSQL owns SQL and default trait methods describe unsupported providers.
- “Add or update tests in the crate that owns the logic.” Live tests skip explicitly without `FASTDB_TEST_PG_PORT` and fail when `FASTDB_REQUIRE_DB_TESTS=1` requires it.
- “Before calling work done: cargo fmt --all, cargo clippy --workspace --all-targets, cargo test --workspace, and run the app for UI changes.” Also build, inspect full diff and untracked files, self-review and independent `code-review-strict` review after the last edit.
- “Do not log connection details.” Administration SQL can contain role credentials; never pass it to SQL observation or script persistence.
- No shared database migrations. Disposable owned PostgreSQL fixture only. Catalog values are parameters; catalog identifiers are quoted by native `format(%I)` or SQL dialect conventions.
- No dependency version/config/message/schema changes. No commits. Retain reusable build dependencies and runnable binary; remove owned fixture and temporary tooling once verification ends.

## Technical Defaults

- Keyset catalog pagination by native OID, 50 displayed objects and one lookahead; existing object browser uses the same identity convention.
- One native parsed statement per operation. PostgreSQL Parse accepts dollar-quoted routine bodies and rejects multiple commands. Reviewed free-form expressions are SQL intentionally; template identifiers are quoted.
- Dedicated transaction and socket per operation, 15-second statement timeout, five-second lock timeout, bounded client deadlines and rollback. Server-enforced privileges, no credential escalation.
- Native text result protocol with 100 retained rows, 32 columns and 512 characters per cell, explicit truncation and NULL/empty distinction. Never retain unbounded results.
- Capture exact SQL for review, prevent editing during review/execution, check driver identity again immediately before dispatch. Escape leaves confirmation first; busy operations allow cancellation and block Quit.

## Risks & Rollback

- Privileged/destructive SQL → exact confirmation and isolated transaction → revert dialog/backend together; committed user database changes require explicit reverse SQL.
- Routine external side effects and sequence `nextval`/`setval` are not rollback-safe → expose limits in outcomes/docs; do not claim all effects were undone.
- Lost commit acknowledgment → report unknown outcome and require refresh before retry; never automatically retry mutations.
- Native catalogs change concurrently → refresh and let native SQL reject changed/missing targets; no claim of atomic object snapshot identity.
- Additive API → defaults preserve existing implementors; remove only this feature's APIs on revert. No saved formats or persisted state.
- Linux tooling environment starts incomplete → approved temporary package extraction; report unavailable verification honestly. macOS manual acceptance cannot be simulated by compilation.

## Scope and design

Complete the PostgreSQL-first administration milestone in `docs/ROADMAP.md`.
Other providers explicitly report administration unsupported through the driver
trait. Use the live profile's transport and credentials; never log them.

Expose **Administration…** on a connected profile. The dialog has paginated
catalog families for routines, sequences, triggers, rules, policies, partitions,
and roles. Show native definitions/details, loading, empty, truncated and error
states. Selecting an object supplies native source and operation templates.
SQL editing uses the existing editor, including multiline bodies and undo.
Templates cover routine replacement and invocation with arguments, sequence
restart, trigger/rule/policy creation and alteration, partition creation,
attachment and detachment, role creation/alteration/removal, membership and
object GRANT/REVOKE. SQL expressions and bodies are intentionally editable SQL,
not silently quoted values. Identifiers generated from catalog rows are quoted.

Require a separate review of the exact SQL before execution. Execute one native-parsed
statement inside a dedicated transaction, with statement/lock deadlines,
cancellation and rollback. Reject transaction/session control so SQL cannot escape
the transaction. Stream result rows and retain only bounded text previews. Do not
persist administration SQL/results or send them to the transaction log (role SQL
may contain secrets). Read-only profiles may browse but cannot execute.

Bind the dialog to its originating driver. Serialize its operations, reject a
replaced/disconnected profile, block underlying keyboard actions and Quit while
execution is in progress. Refresh the selected catalog family after success;
refresh navigator metadata and retain operation/result feedback separately.
Errors preserve editable SQL. Cancellation near commit reports uncertain outcome
and requires refresh before retrying. PostgreSQL enforces object privileges.

## Task 1 — Driver catalogs, templates and execution

Files: `crates/db-wire/src/{administration,lib,traits}.rs`, `crates/db-wire/src/postgres/{administration,mod}.rs`, `crates/db-wire/src/postgres/administration/*.sql`, `crates/db-wire/tests/administration_test.rs`.
Consumes: `ConnectionConfig`, native PostgreSQL catalogs, `flume::Receiver<()>` cancellation. Produces the approved types and trait methods, `postgres_templates(family: AdminFamily, object: Option<&AdminObject>) -> Vec<(&'static str, String)>`, and bounded native results.

- [x] Copy the complete test below into `crates/db-wire/tests/administration_test.rs`. Run `RUSTUP_TOOLCHAIN=stable cargo test --locked -p db-wire --test administration_test --no-run`; expect failure because the inherited draft references eight missing catalog SQL files. Record that the initial draft predates the replacement test-first rules; do not claim a missing-API test was run.
- [x] Apply the complete implementations in Appendix A and the already drafted additive trait/module declarations. Trait signatures: `fn supports_administration(&self) -> bool`; `async fn administration_catalog(&self, family: AdminFamily, after: u32) -> Result<AdminPage>`; `async fn execute_administration(&self, sql: &str, cancel: flume::Receiver<()>) -> Result<AdminOutcome>`. Default false/unsupported; PostgreSQL delegates to its private module.
- [x] Run `cargo test --locked -p db-wire administration`; run the live suite with an owned PostgreSQL fixture and the documented variables. Assert all eight families, real mutations/read-back, limits, SQL NULL, read-only and transaction boundaries. Add focused regressions before correcting a reproduced failure.
- [x] Run EXPLAIN (ANALYZE, BUFFERS) for all eight catalog reads on a catalog with at least 1,000 test relations and routines. Record estimate mismatches, buffer/row counts, sort sizes/spills and correlated lookups. Do not claim a measured speedup.
- [x] Ready to commit the driver files with `feat(db-wire): administer postgresql catalog objects` when requested.

## Task 2 — Administration dialog and integration

Files: new `crates/app/src/workspace/administration.rs`; existing `mod.rs`, `keyboard.rs`, `nav_context_menu.rs`, `script_saves.rs`. Consumes approved driver methods and existing `SqlEditorView`, `ConnectionSession`, `refresh_connection_for`. Produces `open_administration(&mut self, owner: &str, cx: &mut Context<Self>)`, `administration_key(&mut self, key: &Keystroke, cx: &mut Context<Self>)`, `render_administration(&mut self, window: &mut Window, cx: &mut Context<Self>)`.

- [x] Add Appendix B's `ReviewState` test first with the dialog module declaration. Run `cargo test --locked -p fastdb-app administration`; expect missing `ReviewState`. Implement the complete module in Appendix B, then rerun and require PASS.
- [x] Integrate `mod administration;`, a workspace `administration: Option<administration::AdministrationDialog>` initialized to `None`, force workspace focus while open, and render it above the session monitor and below Updates. Add connection-menu `Administration…` gated by `driver.supports_administration()` and idle workspace; dispatch to `open_administration`.
- [x] Route administration keys before the navigator filter and underlying views, respecting the Updates overlay. `request_quit` returns while administration executes; otherwise clear the dialog before existing save/quit logic. No new keybinding; Escape dismisses review first, then closes the idle dialog.
- [x] Test captured SQL, changed SQL, missing review, read-only, stale connection and duplicate execution refusal using `ReviewState`. Verify keyboard order by reading handlers and add model coverage if routing is factored.
- [x] Ready to commit app files with `feat(app): add reviewed postgresql administration` when requested.

## Task 3 — Verification and docs

Files: `docs/guide/navigator-and-tables.md`, `docs/guide/not-built-yet.md`, `docs/PROVIDERS.md`, `docs/ROADMAP.md`, this plan.

- [x] Document the entry point, eight families, editable SQL arguments/bounds/expressions, native privilege enforcement, read-only states, transaction/deadline/cancellation limits and pending desktop acceptance. Mark only implementation and executed automation proven; never imply MySQL/SQLite/ODBC parity or manual acceptance.
- [x] After the last edit run `cargo fmt --all`, `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets`, `cargo test --locked --workspace`, `cargo build --locked --workspace`. Prefix `RUSTUP_TOOLCHAIN=stable` and the temporary tooling paths where required. Run focused live administration tests separately and distinguish skipped cases.
- [x] Launch rebuilt `target/debug/fastdb` with isolated config and a bounded startup interval; inspect process and logs. No Computer Use. Leave the manual checklist pending for the user: all families, source edit/run, sequence reset, triggers/rules/policies, partitions, roles/grants, permissions/read-only, preview dismissal, cancellation, refresh and stale profile.
- [x] Read every changed/untracked file. Self-review requirements/edge cases, concurrency/error paths, bounded performance, credential confidentiality, API compatibility, naming, tests and design with concrete evidence.
- [x] Use `code-review-strict` and its complete package script, then validate findings at file/line. Fix real bugs test-first; rerun gates and re-review any correction. Report model/tool unavailability and every remaining finding.
- [x] Stop/remove only owned fixtures and temporary archives/tooling. Keep reusable Cargo artifacts and runnable binary. Ready to commit docs separately with `docs: describe postgresql administration` when requested.

## Appendix A — Historical driver proposal and live test

Appendices A–C preserve the proposed code before independent review. Use the
checked-in source and final verification below for the current implementation.

### crates/db-wire/src/administration.rs

```rust
//! Provider-neutral administration contracts and PostgreSQL SQL templates.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminFamily {
    Routines,
    Sequences,
    Triggers,
    Rules,
    Policies,
    Partitions,
    Roles,
    Privileges,
}

impl AdminFamily {
    pub const ALL: [Self; 8] = [
        Self::Routines,
        Self::Sequences,
        Self::Triggers,
        Self::Rules,
        Self::Policies,
        Self::Partitions,
        Self::Roles,
        Self::Privileges,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Routines => "Routines",
            Self::Sequences => "Sequences",
            Self::Triggers => "Triggers",
            Self::Rules => "Rules",
            Self::Policies => "Policies",
            Self::Partitions => "Partitions",
            Self::Roles => "Roles",
            Self::Privileges => "Privileges",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AdminObject {
    pub id: u32,
    pub schema: String,
    pub name: String,
    /// Quoted owning relation, or partition parent; empty when inapplicable.
    pub parent: String,
    pub details: String,
    pub source: Option<String>,
    /// Native overload-qualified invocation with editable, typed NULL arguments.
    pub invocation: Option<String>,
}

pub struct AdminPage {
    pub objects: Vec<AdminObject>,
    pub next: Option<u32>,
}

#[derive(Default, Debug)]
pub struct AdminOutcome {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub affected: u64,
    pub truncated: bool,
}

pub const ADMIN_PAGE_SIZE: usize = 50;
pub const ADMIN_SQL_LIMIT: usize = 256 * 1024;
pub const ADMIN_RESULT_ROWS: usize = 100;
pub const ADMIN_RESULT_COLUMNS: usize = 32;
pub const ADMIN_CELL_CHARS: usize = 512;

/// Templates contain intentionally editable SQL expressions. Only catalog-derived
/// identifiers are interpolated; free-form user SQL is reviewed before execution.
pub fn postgres_templates(
    family: AdminFamily,
    object: Option<&AdminObject>,
) -> Vec<(&'static str, String)> {
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let name = object
        .map(|o| quote(&o.name))
        .unwrap_or_else(|| "\"new_object\"".into());
    let target = object
        .map(|o| format!("{}.{}", quote(&o.schema), name))
        .unwrap_or_else(|| "\"public\".\"new_object\"".into());
    let parent = object
        .filter(|o| !o.parent.is_empty())
        .map(|o| o.parent.clone())
        .unwrap_or_else(|| "\"public\".\"target_table\"".into());
    let mut templates = match family {
        AdminFamily::Routines => vec![("New function", "CREATE OR REPLACE FUNCTION \"public\".\"new_function\"(value integer) RETURNS integer LANGUAGE sql AS $body$ SELECT value + 1 $body$;".into()),
            ("New procedure", "CREATE OR REPLACE PROCEDURE \"public\".\"new_procedure\"(value integer) LANGUAGE plpgsql AS $body$ BEGIN RAISE NOTICE 'value: %', value; END $body$;".into())],
        AdminFamily::Sequences => vec![("Create sequence", "CREATE SEQUENCE \"public\".\"new_sequence\" START WITH 1;".into()),
            ("Restart sequence", format!("ALTER SEQUENCE {target} RESTART WITH 1;")),
            ("Alter sequence", format!("ALTER SEQUENCE {target} INCREMENT BY 1 CACHE 1 NO CYCLE;")),
            ("Drop sequence", format!("DROP SEQUENCE {target} RESTRICT;"))],
        AdminFamily::Triggers => vec![("Create trigger", format!("CREATE OR REPLACE TRIGGER \"new_trigger\" BEFORE INSERT ON {parent} FOR EACH ROW EXECUTE FUNCTION \"public\".\"trigger_function\"();")),
            ("Enable trigger", format!("ALTER TABLE {parent} ENABLE TRIGGER {name};")),
            ("Disable trigger", format!("ALTER TABLE {parent} DISABLE TRIGGER {name};")),
            ("Drop trigger", format!("DROP TRIGGER {name} ON {parent} RESTRICT;"))],
        AdminFamily::Rules => vec![("Create rule", format!("CREATE OR REPLACE RULE \"new_rule\" AS ON DELETE TO {parent} DO INSTEAD NOTHING;")),
            ("Enable rule", format!("ALTER TABLE {parent} ENABLE RULE {name};")),
            ("Disable rule", format!("ALTER TABLE {parent} DISABLE RULE {name};")),
            ("Drop rule", format!("DROP RULE {name} ON {parent} RESTRICT;"))],
        AdminFamily::Policies => vec![("Create policy", format!("CREATE POLICY \"new_policy\" ON {parent} FOR SELECT TO PUBLIC USING (true);")),
            ("Alter policy", format!("ALTER POLICY {name} ON {parent} TO PUBLIC USING (true) WITH CHECK (true);")),
            ("Enable RLS", format!("ALTER TABLE {parent} ENABLE ROW LEVEL SECURITY;")),
            ("Disable RLS", format!("ALTER TABLE {parent} DISABLE ROW LEVEL SECURITY;")),
            ("Drop policy", format!("DROP POLICY {name} ON {parent};"))],
        AdminFamily::Partitions => {
            let root = object.filter(|o| o.parent.is_empty()).map(|_| target.clone()).unwrap_or(parent);
            vec![("Create partition", format!("CREATE TABLE \"public\".\"new_partition\" PARTITION OF {root} FOR VALUES FROM (0) TO (100);")),
                ("Attach partition", format!("ALTER TABLE {root} ATTACH PARTITION \"public\".\"existing_table\" FOR VALUES FROM (0) TO (100);")),
                ("Detach partition", format!("ALTER TABLE {root} DETACH PARTITION {target};"))]
        }
        AdminFamily::Roles => vec![("Create role", "CREATE ROLE \"new_role\" NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;".into()),
            ("Create user", "CREATE ROLE \"new_user\" LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;".into()),
            ("Allow login", format!("ALTER ROLE {name} LOGIN;")),
            ("Disable login", format!("ALTER ROLE {name} NOLOGIN;")),
            ("Grant membership", format!("GRANT {name} TO \"member_role\";")),
            ("Revoke membership", format!("REVOKE {name} FROM \"member_role\";")),
            ("Drop role", format!("DROP ROLE {name};"))],
        AdminFamily::Privileges => vec![("Grant privilege", format!("GRANT SELECT ON TABLE {target} TO \"recipient_role\";")),
            ("Revoke privilege", format!("REVOKE SELECT ON TABLE {target} FROM \"recipient_role\" RESTRICT;")),
            ("Grant schema usage", "GRANT USAGE ON SCHEMA \"public\" TO \"recipient_role\";".into()),
            ("Grant routine execute", "GRANT EXECUTE ON FUNCTION \"public\".\"function_name\"(integer) TO \"recipient_role\";".into()),
            ("Grant sequence usage", "GRANT USAGE ON SEQUENCE \"public\".\"sequence_name\" TO \"recipient_role\";".into())],
    };
    if let Some(source) = object.and_then(|o| o.source.clone()) {
        templates.insert(0, ("Edit source", source));
    }
    if let Some(sql) = object.and_then(|o| o.invocation.clone()) {
        templates.push(("Run with arguments", sql));
    }
    templates
}

/// PostgreSQL validates the complete statement with Parse before execution.
/// A narrow first-keyword allowlist excludes transaction/session commands even
/// when comments precede them. Comments may instead follow the first keyword.
pub fn validate_admin_sql(sql: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !sql.contains('\0') && sql.len() <= ADMIN_SQL_LIMIT,
        "SQL must contain no NUL and be at most 256 KiB"
    );
    let first = sql
        .trim_start()
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    anyhow::ensure!(
        matches!(
            first.as_str(),
            "CREATE" | "ALTER" | "DROP" | "GRANT" | "REVOKE" | "COMMENT" | "SELECT" | "CALL"
        ),
        "Start with CREATE, ALTER, DROP, GRANT, REVOKE, COMMENT, SELECT or CALL; execute one statement at a time"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn templates_quote_catalog_identifiers_and_preserve_native_invocation() {
        let object = AdminObject {
            id: 1,
            schema: "odd\"schema".into(),
            name: "x\"; DROP ROLE y; --".into(),
            parent: "\"s\".\"table\"".into(),
            details: String::new(),
            source: None,
            invocation: Some("SELECT \"s\".\"f\"(NULL::integer);".into()),
        };
        let sequence = postgres_templates(AdminFamily::Sequences, Some(&object));
        assert_eq!(
            sequence[1].1,
            "ALTER SEQUENCE \"odd\"\"schema\".\"x\"\"; DROP ROLE y; --\" RESTART WITH 1;"
        );
        assert_eq!(sequence.last().unwrap().1, object.invocation.unwrap());
        assert!(
            postgres_templates(
                AdminFamily::Triggers,
                Some(&AdminObject {
                    invocation: None,
                    ..object
                })
            )
            .iter()
            .any(|(_, sql)| sql.starts_with("ALTER TABLE \"s\".\"table\" DISABLE TRIGGER"))
        );
    }
    #[test]
    fn transaction_controls_and_oversized_sql_are_rejected() {
        for sql in [
            "COMMIT",
            "ROLLBACK",
            "SET statement_timeout=0",
            "/* comment */ COMMIT",
            "-- comment\nEND",
            "",
            "SELECT '\0'",
        ] {
            assert!(validate_admin_sql(sql).is_err(), "{sql:?}");
        }
        assert!(validate_admin_sql(&format!("SELECT '{}'", "x".repeat(ADMIN_SQL_LIMIT))).is_err());
        assert!(
            validate_admin_sql(
                "CREATE FUNCTION f() RETURNS void LANGUAGE plpgsql AS $$ BEGIN NULL; END $$;"
            )
            .is_ok()
        );
    }
}

```

### crates/db-wire/src/postgres/administration.rs

```rust
use super::{EstablishedConnection, PostgresDriver, SocketGuard, pg_error_message};
use crate::administration::*;
use anyhow::{Context, Result};
use futures::TryStreamExt;
use std::{sync::Arc, time::Duration};
use tokio_postgres::{Client, SimpleQueryMessage};

fn catalog_sql(family: AdminFamily) -> &'static str {
    match family {
        AdminFamily::Routines => include_str!("administration/routines.sql"),
        AdminFamily::Sequences => include_str!("administration/sequences.sql"),
        AdminFamily::Triggers => include_str!("administration/triggers.sql"),
        AdminFamily::Rules => include_str!("administration/rules.sql"),
        AdminFamily::Policies => include_str!("administration/policies.sql"),
        AdminFamily::Partitions => include_str!("administration/partitions.sql"),
        AdminFamily::Roles => include_str!("administration/roles.sql"),
        AdminFamily::Privileges => include_str!("administration/privileges.sql"),
    }
}

impl PostgresDriver {
    pub(super) async fn read_administration_catalog(
        &self,
        family: AdminFamily,
        after: u32,
    ) -> Result<AdminPage> {
        tokio::time::timeout(Duration::from_secs(15), async {
            let (client, task) = Self::connect_with_task(self.config.clone()).await?;
            let _socket = SocketGuard(task);
            client.batch_execute("SET search_path=pg_catalog; SET statement_timeout='10s'; BEGIN READ ONLY").await?;
            let sql = format!("SELECT id, schema, name, parent, left(details,8192), CASE WHEN octet_length(source)<=$3 THEN source END, invocation FROM ({}) AS objects WHERE id>$1 ORDER BY id LIMIT $2", catalog_sql(family));
            let rows = client.query(&sql, &[&after, &((ADMIN_PAGE_SIZE + 1) as i64), &(ADMIN_SQL_LIMIT as i32)]).await
                .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
            let more = rows.len() > ADMIN_PAGE_SIZE;
            let objects: Vec<_> = rows.into_iter().take(ADMIN_PAGE_SIZE).map(|row| AdminObject {
                id: row.get(0), schema: row.get(1), name: row.get(2), parent: row.get(3),
                details: row.get(4), source: row.get(5), invocation: row.get(6),
            }).collect();
            let next = more.then(|| objects.last().unwrap().id);
            Ok(AdminPage { objects, next })
        }).await.context("Administration catalog timed out")?
    }

    pub(super) async fn run_administration(
        &self,
        sql: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<AdminOutcome> {
        anyhow::ensure!(
            !self.config.read_only,
            "Administration execution is disabled for read-only profiles"
        );
        validate_admin_sql(sql)?;
        let EstablishedConnection {
            client,
            task,
            cancellation,
        } = tokio::select! {
            biased;
            _ = cancel.recv_async() => anyhow::bail!("Administration canceled before connecting"),
            connection = Self::connect_with_notices(self.config.clone(), Arc::default()) => connection?,
        };
        let _socket = SocketGuard(task);
        let result = tokio::select! {
            biased;
            _ = cancel.recv_async() => Err(anyhow::anyhow!("Administration canceled")),
            result = tokio::time::timeout(Duration::from_secs(20), execute_statement(&client, sql)) => {
                result.unwrap_or_else(|_| Err(anyhow::anyhow!("Administration timed out")))
            }
        };
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                let cancellation_status =
                    match tokio::time::timeout(Duration::from_secs(2), cancellation.cancel()).await
                    {
                        Ok(Ok(())) => "Server cancellation acknowledged",
                        Ok(Err(_)) => "Server cancellation failed",
                        Err(_) => "Server cancellation timed out",
                    };
                let rollback =
                    tokio::time::timeout(Duration::from_secs(3), client.batch_execute("ROLLBACK"))
                        .await;
                return match rollback {
                    Ok(Ok(())) => Err(error.context(format!("{cancellation_status}. Transaction rolled back; external routine effects and sequence nextval/setval may persist"))),
                    _ => Err(error.context("Connection closed; verify server state before retrying")),
                };
            }
        };
        if cancel.try_recv().is_ok() || cancel.is_disconnected() {
            tokio::time::timeout(Duration::from_secs(3), client.batch_execute("ROLLBACK"))
                .await
                .context("Rollback timed out; verify server state")??;
            anyhow::bail!(
                "Administration canceled and rolled back; external routine effects and sequence nextval/setval may persist"
            );
        }
        // Once COMMIT is sent, cancellation cannot establish whether it committed.
        tokio::time::timeout(Duration::from_secs(5), client.batch_execute("COMMIT"))
            .await
            .context("Commit outcome unknown; refresh before retrying")?
            .map_err(|_| anyhow::anyhow!("Commit was not acknowledged; refresh before retrying"))?;
        Ok(outcome)
    }
}

async fn execute_statement(client: &Client, sql: &str) -> Result<AdminOutcome> {
    client.batch_execute("SET search_path=pg_catalog,public; SET statement_timeout='15s'; SET lock_timeout='5s'; BEGIN").await?;
    // Parse rejects multiple statements, including COMMIT injected after a DDL
    // statement. Unlike splitting on semicolons, it accepts routine dollar bodies.
    let statement = client
        .prepare(sql)
        .await
        .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
    anyhow::ensure!(
        statement.params().is_empty(),
        "Replace argument placeholders with SQL values before reviewing"
    );
    let stream = client
        .simple_query_raw(sql)
        .await
        .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
    futures::pin_mut!(stream);
    let mut outcome = AdminOutcome::default();
    while let Some(message) = stream
        .try_next()
        .await
        .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?
    {
        match message {
            SimpleQueryMessage::RowDescription(columns) => {
                outcome.truncated |= columns.len() > ADMIN_RESULT_COLUMNS;
                outcome.columns = columns
                    .iter()
                    .take(ADMIN_RESULT_COLUMNS)
                    .map(|c| c.name().to_owned())
                    .collect();
            }
            SimpleQueryMessage::Row(row) => {
                if outcome.rows.len() < ADMIN_RESULT_ROWS {
                    let values = (0..row.len().min(ADMIN_RESULT_COLUMNS))
                        .map(|i| {
                            row.get(i).map(|value| {
                                outcome.truncated |= value.chars().count() > ADMIN_CELL_CHARS;
                                value.chars().take(ADMIN_CELL_CHARS).collect()
                            })
                        })
                        .collect();
                    outcome.rows.push(values);
                } else {
                    outcome.truncated = true;
                }
            }
            SimpleQueryMessage::CommandComplete(count) => outcome.affected = count,
            _ => {}
        }
    }
    Ok(outcome)
}

```

### crates/db-wire/tests/administration_test.rs

```rust
use db_wire::administration::{ADMIN_PAGE_SIZE, ADMIN_RESULT_ROWS, AdminFamily};
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver};
use futures::FutureExt;
use std::time::Duration;

fn config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return None;
    };
    let env = |name| std::env::var(format!("FASTDB_TEST_PG_{name}")).ok();
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: env("HOST").unwrap_or_else(|| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        database: env("DATABASE").unwrap_or_else(|| "postgres".into()),
        user: env("USER").unwrap_or_else(|| "postgres".into()),
        password: env("PASSWORD"),
        use_unix_socket: false,
        connect_timeout: Duration::from_secs(5),
        ..Default::default()
    })
}
async fn run(driver: &PostgresDriver, sql: &str) -> db_wire::administration::AdminOutcome {
    let (_send, cancel) = flume::bounded(1);
    driver
        .execute_administration(sql, cancel)
        .await
        .unwrap_or_else(|e| panic!("administration fixture failed: {e:#}"))
}

#[test]
fn postgres_administration_mutates_catalogs_and_preserves_transaction_boundaries() {
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
            let driver = PostgresDriver::new(config.clone());
            // Dedicated schema and roles are unique per invocation, not shared fixtures.
            let suffix = uuid::Uuid::new_v4().simple().to_string();
            let schema = format!("admin_{suffix}");
            let role = format!("admin_role_{suffix}");
            let member = format!("admin_member_{suffix}");
            run(&driver, &format!("CREATE SCHEMA \"{schema}\";")).await;
            let result = tokio::time::timeout(Duration::from_secs(70), std::panic::AssertUnwindSafe(async {
                run(&driver, &format!("CREATE FUNCTION \"{schema}\".f(value integer) RETURNS integer LANGUAGE sql AS $$ SELECT value+1 $$;")).await;
                assert_eq!(run(&driver, &format!("SELECT \"{schema}\".f(5)")).await.rows, vec![vec![Some("6".into())]]);
                run(&driver, &format!("CREATE OR REPLACE FUNCTION \"{schema}\".f(value integer) RETURNS integer LANGUAGE sql AS $$ SELECT value+2 $$;")).await;
                assert_eq!(run(&driver, &format!("SELECT \"{schema}\".f(5)")).await.rows[0][0].as_deref(), Some("7"));
                run(&driver, &format!("CREATE FUNCTION \"{schema}\".variadic_f(OUT value integer, VARIADIC values_in integer[]) LANGUAGE sql AS $$ SELECT coalesce(array_length(values_in,1),0) $$")).await;
                run(&driver, &format!("CREATE PROCEDURE \"{schema}\".p(value integer) LANGUAGE plpgsql AS $$ BEGIN PERFORM value; END $$")).await;
                run(&driver, &format!("CALL \"{schema}\".p(9)")).await;
                run(&driver, &format!("CREATE SEQUENCE \"{schema}\".s")).await;
                run(&driver, &format!("ALTER SEQUENCE \"{schema}\".s RESTART WITH 42")).await;
                assert_eq!(run(&driver, &format!("SELECT nextval('\"{schema}\".s')")).await.rows[0][0].as_deref(), Some("42"));
                run(&driver, &format!("CREATE TABLE \"{schema}\".t(id integer)")).await;
                run(&driver, &format!("CREATE FUNCTION \"{schema}\".tf() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$")).await;
                run(&driver, &format!("CREATE TRIGGER tr BEFORE INSERT ON \"{schema}\".t FOR EACH ROW EXECUTE FUNCTION \"{schema}\".tf()")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".t DISABLE TRIGGER tr")).await;
                run(&driver, &format!("CREATE RULE rule1 AS ON DELETE TO \"{schema}\".t DO INSTEAD NOTHING")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".t DISABLE RULE rule1")).await;
                run(&driver, &format!("CREATE POLICY policy1 ON \"{schema}\".t FOR SELECT USING (id>0)")).await;
                run(&driver, &format!("ALTER POLICY policy1 ON \"{schema}\".t USING (id>1)")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".t ENABLE ROW LEVEL SECURITY")).await;
                run(&driver, &format!("CREATE TABLE \"{schema}\".root(id integer) PARTITION BY RANGE(id)")).await;
                run(&driver, &format!("CREATE TABLE \"{schema}\".child PARTITION OF \"{schema}\".root FOR VALUES FROM (0) TO (10)")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".root DETACH PARTITION \"{schema}\".child")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".root ATTACH PARTITION \"{schema}\".child FOR VALUES FROM (0) TO (10)")).await;
                run(&driver, &format!("CREATE ROLE \"{role}\" NOLOGIN")).await;
                run(&driver, &format!("CREATE ROLE \"{member}\" NOLOGIN")).await;
                run(&driver, &format!("GRANT \"{role}\" TO \"{member}\"")).await;
                run(&driver, &format!("GRANT SELECT ON \"{schema}\".t TO \"{role}\"")).await;
                let pagination_fixture = (0..55).map(|i| format!("CREATE SEQUENCE \"{schema}\".page_{i};")).collect::<String>();
                driver.execute_schema_ddl(&pagination_fixture).await.unwrap();
                for family in AdminFamily::ALL {
                    let mut after = 0; let mut found = false; let mut pages = 0;
                    loop {
                        let page = driver.administration_catalog(family, after).await.unwrap();
                        pages += 1;
                        assert!(page.objects.len() <= ADMIN_PAGE_SIZE);
                        for object in &page.objects {
                            found |= if family == AdminFamily::Roles { object.name == role } else { object.schema == schema };
                            if object.schema == schema && family == AdminFamily::Routines && object.name == "f" {
                                assert!(object.invocation.as_ref().unwrap().contains("NULL::integer"));
                                assert!(object.source.as_ref().unwrap().contains("value+2"));
                            }
                            if object.schema == schema && family == AdminFamily::Routines && object.name == "variadic_f" {
                                let sql = object.invocation.as_ref().unwrap();
                                assert!(sql.contains("VARIADIC NULL::integer[]"));
                                assert_eq!(run(&driver,sql).await.rows[0][0].as_deref(),Some("0"));
                            }
                            if object.schema == schema && matches!(family, AdminFamily::Triggers | AdminFamily::Rules) {
                                assert!(object.details.contains("enabled: D"));
                            }
                            if object.schema == schema && family == AdminFamily::Partitions && object.name == "child" {
                                assert_eq!(object.parent,format!("{schema}.root"));
                                assert!(object.details.contains("FOR VALUES FROM (0) TO (10)"));
                            }
                            if family == AdminFamily::Roles && object.name == role { assert!(object.details.contains("login: f")); }
                            if object.schema == schema && family == AdminFamily::Privileges && object.name == "t" { assert!(object.details.contains(&role)); }
                            if object.schema == schema && let Some(source) = &object.source {
                                run(&driver,source).await;
                            }
                            if object.schema == schema && family == AdminFamily::Policies {
                                assert!(object.source.as_ref().unwrap().contains("id > 1"));
                            }
                        }
                        if let Some(next) = page.next { assert!(next > after); after = next; } else { break; }
                    }
                    assert!(found, "missing family {family:?}");
                    if family == AdminFamily::Sequences { assert!(pages >= 2); }
                }
                let empty = driver.administration_catalog(AdminFamily::Roles,u32::MAX).await.unwrap();
                assert!(empty.objects.is_empty()); assert!(empty.next.is_none());
                run(&driver, &format!("REVOKE \"{role}\" FROM \"{member}\"")).await;
                assert_eq!(run(&driver, &format!("SELECT pg_has_role('{member}','{role}','MEMBER')")).await.rows[0][0].as_deref(),Some("f"));
                assert_eq!(run(&driver, &format!("SELECT has_table_privilege('{role}','\"{schema}\".t','SELECT')")).await.rows[0][0].as_deref(), Some("t"));
                run(&driver, &format!("REVOKE SELECT ON \"{schema}\".t FROM \"{role}\"")).await;
                assert_eq!(run(&driver, &format!("SELECT has_table_privilege('{role}','\"{schema}\".t','SELECT')")).await.rows[0][0].as_deref(), Some("f"));
                let outcome = run(&driver, "SELECT generate_series(1,150),NULL::text,''::text").await;
                assert_eq!(outcome.rows.len(), ADMIN_RESULT_ROWS); assert!(outcome.truncated);
                assert_eq!(outcome.rows[0][1], None); assert_eq!(outcome.rows[0][2].as_deref(), Some(""));
                let outcome = run(&driver, "SELECT repeat('界',600)").await;
                assert!(outcome.truncated);
                assert_eq!(outcome.rows[0][0].as_ref().unwrap().chars().count(),512);
                let sql = format!("SELECT {}", (0..40).map(|i| format!("{i} AS c{i}")).collect::<Vec<_>>().join(","));
                let outcome = run(&driver,&sql).await;
                assert_eq!(outcome.columns.len(),32);
                assert_eq!(outcome.rows[0].len(),32);
                assert!(outcome.truncated);
                let (_send, cancel) = flume::bounded(1);
                assert!(driver.execute_administration(&format!("CREATE TABLE \"{schema}\".escaped(id int); COMMIT;"), cancel).await.is_err());
                assert_eq!(run(&driver, &format!("SELECT to_regclass('\"{schema}\".escaped') IS NULL")).await.rows[0][0].as_deref(), Some("t"));
                let mut readonly = config.clone(); readonly.read_only = true;
                let readonly = PostgresDriver::new(readonly);
                assert!(!readonly.administration_catalog(AdminFamily::Roles, 0).await.unwrap().objects.is_empty());
                let (_send, cancel) = flume::bounded(1);
                assert!(readonly.execute_administration("SELECT 1", cancel).await.is_err());
                let (send, cancel) = flume::bounded(1); send.send(()).unwrap();
                assert!(driver.execute_administration("SELECT pg_sleep(10)", cancel).await.unwrap_err().to_string().contains("canceled"));
                // Native privilege failure must preserve catalog state.
                let password = uuid::Uuid::new_v4().simple().to_string();
                run(&driver, &format!("ALTER ROLE \"{role}\" LOGIN PASSWORD '{password}'")).await;
                let mut restricted = config.clone(); restricted.user = role.clone(); restricted.password = Some(password);
                let restricted = PostgresDriver::new(restricted);
                let (_send, cancel) = flume::bounded(1);
                let error = restricted.execute_administration(&format!("CREATE TABLE \"{schema}\".denied(id int)"), cancel).await.unwrap_err();
                assert!(format!("{error:#}").contains("permission denied"));
                assert_eq!(run(&driver, &format!("SELECT to_regclass('\"{schema}\".denied') IS NULL")).await.rows[0][0].as_deref(), Some("t"));
                // Wait on server-observed PgSleep, not a timing assumption.
                run(&driver, &format!("CREATE PROCEDURE \"{schema}\".slow() LANGUAGE plpgsql AS $$ BEGIN INSERT INTO \"{schema}\".t VALUES(5); PERFORM pg_sleep(10); END $$")).await;
                let mut cancellable = config.clone(); cancellable.application_name = Some(format!("cancel_{suffix}"));
                let cancellable = PostgresDriver::new(cancellable);
                let (send, cancel) = flume::bounded(1);
                let sql = format!("CALL \"{schema}\".slow()");
                let task = tokio::spawn(async move { cancellable.execute_administration(&sql,cancel).await });
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let observed = run(&driver, &format!("SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE application_name='cancel_{suffix}' AND wait_event='PgSleep'")).await;
                        if observed.rows[0][0].as_deref() == Some("1") { break; }
                        tokio::task::yield_now().await;
                    }
                }).await.unwrap();
                send.send(()).unwrap();
                let error = task.await.unwrap().unwrap_err();
                assert!(format!("{error:#}").contains("rolled back"));
                assert_eq!(run(&driver, &format!("SELECT count(*) FROM \"{schema}\".t")).await.rows[0][0].as_deref(), Some("0"));
                anyhow::Ok(())
            }).catch_unwind()).await;
            // Cleanup also runs after assertion panics or a timed-out fixture body.
            run(&driver, &format!("DROP SCHEMA \"{schema}\" CASCADE")).await;
            run(&driver, &format!("DROP ROLE IF EXISTS \"{member}\"")).await;
            run(&driver, &format!("DROP ROLE IF EXISTS \"{role}\"")).await;
            match result {
                Ok(Ok(result)) => result.unwrap(),
                Ok(Err(panic)) => std::panic::resume_unwind(panic),
                Err(error) => panic!("administration fixture timed out: {error}"),
            }
    });
}

#[test]
fn administration_is_explicitly_unsupported_for_sqlite() {
    let driver = db_wire::create_driver(ConnectionConfig {
        driver: DbDriverKind::Sqlite,
        database: ":memory:".into(),
        ..Default::default()
    })
    .unwrap();
    assert!(!driver.supports_administration());
    db_wire::get_runtime().block_on(async {
        let error = driver
            .administration_catalog(AdminFamily::Roles, 0)
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("not supported"));
        let (_send, cancel) = flume::bounded(1);
        let error = driver
            .execute_administration("SELECT 1", cancel)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not supported"));
    });
}

```

### /tmp/fastdb-p201-plan/administration/partitions.sql

```sql
SELECT c.oid AS id,n.nspname::text AS schema,c.relname::text AS name,
 CASE WHEN c.relispartition THEN format('%I.%I',pn.nspname,pc.relname) ELSE '' END AS parent,
 format('partition key: %s; bound: %s; owner: %s',coalesce(pg_get_partkeydef(c.oid),'not partitioned'),
 coalesce(pg_get_expr(c.relpartbound,c.oid),'root'),pg_get_userbyid(c.relowner)) AS details,
 NULL::text AS source,NULL::text AS invocation FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 LEFT JOIN pg_inherits i ON i.inhrelid=c.oid AND c.relispartition
 LEFT JOIN pg_class pc ON pc.oid=i.inhparent LEFT JOIN pg_namespace pn ON pn.oid=pc.relnamespace
 WHERE (c.relkind='p' OR c.relispartition) AND n.nspname !~ '^pg_(toast|temp)'

```

### /tmp/fastdb-p201-plan/administration/policies.sql

```sql
SELECT p.oid AS id,n.nspname::text AS schema,p.polname::text AS name,format('%I.%I',n.nspname,c.relname) AS parent,
 format('table: %I.%I; command: %s; permissive: %s; RLS enabled: %s; RLS forced: %s',n.nspname,c.relname,p.polcmd,p.polpermissive,c.relrowsecurity,c.relforcerowsecurity) AS details,
 format('ALTER POLICY %I ON %I.%I TO %s%s%s;',p.polname,n.nspname,c.relname,
 (SELECT string_agg(CASE WHEN roleid=0 THEN 'PUBLIC' ELSE quote_ident(pg_get_userbyid(roleid)) END,', ' ORDER BY roleid) FROM unnest(p.polroles) roleid),
 CASE WHEN p.polqual IS NULL THEN '' ELSE ' USING (' || pg_get_expr(p.polqual,p.polrelid) || ')' END,
 CASE WHEN p.polwithcheck IS NULL THEN '' ELSE ' WITH CHECK (' || pg_get_expr(p.polwithcheck,p.polrelid) || ')' END) AS source,
 NULL::text AS invocation FROM pg_policy p JOIN pg_class c ON c.oid=p.polrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname !~ '^pg_(toast|temp)'

```

### /tmp/fastdb-p201-plan/administration/privileges.sql

```sql
SELECT c.oid AS id,n.nspname::text AS schema,c.relname::text AS name,''::text AS parent,
 format('owner: %s; explicit ACL (NULL means native defaults): %s',pg_get_userbyid(c.relowner),coalesce(c.relacl::text,'NULL')) AS details,
 NULL::text AS source,NULL::text AS invocation FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE c.relkind IN ('r','p','v','m','f') AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_(toast|temp)'

```

### /tmp/fastdb-p201-plan/administration/roles.sql

```sql
SELECT r.oid AS id,''::text AS schema,r.rolname::text AS name,''::text AS parent,
 format('login: %s; superuser: %s; create DB: %s; create role: %s; inherit: %s; replication: %s; bypass RLS: %s; connection limit: %s; member of: %s',
 r.rolcanlogin,r.rolsuper,r.rolcreatedb,r.rolcreaterole,r.rolinherit,r.rolreplication,r.rolbypassrls,r.rolconnlimit,
 coalesce((SELECT string_agg(quote_ident(pg_get_userbyid(m.roleid)),', ' ORDER BY m.roleid) FROM pg_auth_members m WHERE m.member=r.oid),'none')) AS details,
 format('ALTER ROLE %I %s %s CONNECTION LIMIT %s;',r.rolname,CASE WHEN r.rolcanlogin THEN 'LOGIN' ELSE 'NOLOGIN' END,
 CASE WHEN r.rolinherit THEN 'INHERIT' ELSE 'NOINHERIT' END,r.rolconnlimit) AS source,NULL::text AS invocation FROM pg_roles r

```

### /tmp/fastdb-p201-plan/administration/routines.sql

```sql
SELECT p.oid AS id, n.nspname::text AS schema, p.proname::text AS name,
 ''::text AS parent,
 format('%s(%s) → %s; owner: %s', CASE p.prokind WHEN 'p' THEN 'procedure' ELSE 'function' END,
 pg_get_function_identity_arguments(p.oid), pg_get_function_result(p.oid), pg_get_userbyid(p.proowner)) AS details,
 pg_get_functiondef(p.oid) AS source,
 format('%s %I.%I(%s);', CASE p.prokind WHEN 'p' THEN 'CALL' ELSE 'SELECT * FROM' END,
 n.nspname,p.proname,coalesce((SELECT string_agg(
 CASE WHEN p.provariadic<>0 AND a.ordinality=array_length(CASE WHEN p.prokind='p' THEN coalesce(p.proallargtypes,p.proargtypes::oid[]) ELSE p.proargtypes::oid[] END,1) THEN 'VARIADIC ' ELSE '' END
 || 'NULL::' || format_type(a.typ,NULL), ', ' ORDER BY a.ordinality)
 FROM unnest(CASE WHEN p.prokind='p' THEN coalesce(p.proallargtypes,p.proargtypes::oid[]) ELSE p.proargtypes::oid[] END)
 WITH ORDINALITY a(typ,ordinality)),'')) AS invocation
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE p.prokind IN ('f','p') AND n.nspname NOT IN ('pg_catalog','information_schema')
 AND n.nspname !~ '^pg_(toast|temp)'

```

### /tmp/fastdb-p201-plan/administration/rules.sql

```sql
SELECT r.oid AS id,n.nspname::text AS schema,r.rulename::text AS name,format('%I.%I',n.nspname,c.relname) AS parent,
 format('relation: %I.%I; enabled: %s',n.nspname,c.relname,r.ev_enabled) AS details,
 regexp_replace(pg_get_ruledef(r.oid,true),'^CREATE RULE','CREATE OR REPLACE RULE') AS source,
 NULL::text AS invocation FROM pg_rewrite r JOIN pg_class c ON c.oid=r.ev_class JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE r.rulename<>'_RETURN' AND n.nspname !~ '^pg_(toast|temp)'

```

### /tmp/fastdb-p201-plan/administration/sequences.sql

```sql
SELECT c.oid AS id,n.nspname::text AS schema,c.relname::text AS name,''::text AS parent,
 format('type: %s; start: %s; increment: %s; min: %s; max: %s; cache: %s; cycle: %s; owner: %s; last value: %s',
 format_type(s.seqtypid,NULL),s.seqstart,s.seqincrement,s.seqmin,s.seqmax,s.seqcache,s.seqcycle,
 pg_get_userbyid(c.relowner),pg_sequence_last_value(c.oid)) AS details,
 format('ALTER SEQUENCE %I.%I INCREMENT BY %s MINVALUE %s MAXVALUE %s START WITH %s CACHE %s %s;',
 n.nspname,c.relname,s.seqincrement,s.seqmin,s.seqmax,s.seqstart,s.seqcache,CASE WHEN s.seqcycle THEN 'CYCLE' ELSE 'NO CYCLE' END) AS source,
 NULL::text AS invocation FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_sequence s ON s.seqrelid=c.oid
 WHERE n.nspname !~ '^pg_(toast|temp)'

```

### /tmp/fastdb-p201-plan/administration/triggers.sql

```sql
SELECT t.oid AS id,n.nspname::text AS schema,t.tgname::text AS name,format('%I.%I',n.nspname,c.relname) AS parent,
 format('table: %I.%I; enabled: %s',n.nspname,c.relname,t.tgenabled) AS details,
 regexp_replace(pg_get_triggerdef(t.oid,true),'^CREATE TRIGGER','CREATE OR REPLACE TRIGGER') || ';' AS source,
 NULL::text AS invocation FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE NOT t.tgisinternal AND n.nspname !~ '^pg_(toast|temp)'

```

## Appendix B — Historical app module and model test

```rust
//! Connection-bound PostgreSQL catalog administration and reviewed SQL.
use super::script_tools::tool;
use super::*;
use db_wire::administration::{
    AdminFamily, AdminObject, AdminPage, postgres_templates, validate_admin_sql,
};
use std::time::Duration;

#[derive(Default)]
struct ReviewState {
    sql: Option<String>,
    busy: bool,
}
impl ReviewState {
    fn review(&mut self, sql: String) -> anyhow::Result<()> {
        anyhow::ensure!(!self.busy, "An operation is already running");
        validate_admin_sql(&sql)?;
        self.sql = Some(sql);
        Ok(())
    }
    fn take(&mut self, current: &str, writable: bool, live: bool) -> anyhow::Result<String> {
        anyhow::ensure!(
            !self.busy && writable && live,
            "The original writable connection must be available and idle"
        );
        let sql = self
            .sql
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Review SQL first"))?;
        anyhow::ensure!(sql == current, "SQL changed; review it again");
        self.busy = true;
        Ok(self.sql.take().unwrap())
    }
}

pub(super) struct AdministrationDialog {
    id: u64,
    owner: String,
    driver: Arc<dyn DatabaseDriver>,
    family: AdminFamily,
    objects: Vec<AdminObject>,
    selected: Option<usize>,
    next: Option<u32>,
    loading: bool,
    review: ReviewState,
    editor: SqlEditorView,
    templates: Vec<(&'static str, String)>,
    message: String,
    result: String,
    cancel: Option<flume::Sender<()>>,
}
impl AdministrationDialog {
    pub(super) fn executing(&self) -> bool {
        self.review.busy
    }
    fn editable(&self) -> bool {
        !self.review.busy && self.review.sql.is_none()
    }
    fn set_templates(&mut self) {
        self.templates =
            postgres_templates(self.family, self.selected.and_then(|i| self.objects.get(i)));
    }
}

impl Workspace {
    fn administration_live(&self) -> bool {
        self.administration.as_ref().is_some_and(|dialog| {
            self.connections
                .sessions
                .get(&dialog.owner)
                .and_then(|s| s.driver.as_ref())
                .is_some_and(|d| Arc::ptr_eq(d, &dialog.driver))
        })
    }
    pub(super) fn open_administration(&mut self, owner: &str, cx: &mut Context<Self>) {
        if self.administration.is_some() || self.operations.is_running() {
            return;
        }
        let Some(driver) = self
            .connections
            .sessions
            .get(owner)
            .and_then(|s| s.driver.clone())
        else {
            return;
        };
        if !driver.supports_administration() {
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.administration = Some(AdministrationDialog {
            id,
            owner: owner.into(),
            driver,
            family: AdminFamily::Routines,
            objects: Vec::new(),
            selected: None,
            next: None,
            loading: false,
            review: ReviewState::default(),
            editor: SqlEditorView::new("", self.theme.clone(), None),
            templates: Vec::new(),
            message: String::new(),
            result: String::new(),
            cancel: None,
        });
        self.load_administration(AdminFamily::Routines, 0, cx);
    }
    fn load_administration(&mut self, family: AdminFamily, after: u32, cx: &mut Context<Self>) {
        let live = self.administration_live();
        let Some(dialog) = &mut self.administration else {
            return;
        };
        if dialog.loading || dialog.review.busy || dialog.review.sql.is_some() {
            return;
        }
        if !live {
            dialog.message = "Original connection changed. Close and reopen Administration.".into();
            cx.notify();
            return;
        }
        dialog.family = family;
        dialog.objects.clear();
        dialog.selected = None;
        dialog.next = None;
        dialog.loading = true;
        dialog.message = "Loading catalog…".into();
        dialog.set_templates();
        let id = dialog.id;
        let driver = dialog.driver.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    db_wire::get_runtime().block_on(async {
                        tokio::time::timeout(
                            Duration::from_secs(16),
                            driver.administration_catalog(family, after),
                        )
                        .await
                        .map_err(|_| anyhow::anyhow!("Catalog request timed out"))?
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let live = this.administration_live();
                let Some(dialog) = this.administration.as_mut().filter(|d| d.id == id) else {
                    return;
                };
                dialog.loading = false;
                if !live {
                    dialog.message =
                        "Original connection changed. Close and reopen Administration.".into();
                } else {
                    match result {
                        Ok(AdminPage { objects, next }) => {
                            dialog.message = if objects.is_empty() {
                                "No objects found.".into()
                            } else if next.is_some() {
                                "More objects available: use Next page.".into()
                            } else {
                                "Catalog loaded. Select an object or choose a creation template."
                                    .into()
                            };
                            dialog.objects = objects;
                            dialog.next = next;
                        }
                        Err(error) => dialog.message = format!("Catalog failed: {error:#}"),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn administration_select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.administration else {
            return;
        };
        if !dialog.editable() || dialog.loading {
            return;
        }
        let Some(object) = dialog.objects.get(index) else {
            return;
        };
        dialog.message = object.details.clone();
        if object.source.is_none() {
            dialog.message.push_str("\nNo editable native source is available for this object; choose an operation template.");
        }
        dialog.selected = Some(index);
        dialog.set_templates();
        if let Some((_, sql)) = dialog.templates.first() {
            dialog.editor = SqlEditorView::new(sql, self.theme.clone(), None);
        }
        cx.notify();
    }
    fn administration_review(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.administration else {
            return;
        };
        if let Err(error) = dialog.review.review(dialog.editor.buffer.text()) {
            dialog.result = error.to_string();
        }
        cx.notify();
    }
    fn administration_execute(&mut self, cx: &mut Context<Self>) {
        let live = self.administration_live() && !self.operations.is_running();
        let Some(dialog) = &mut self.administration else {
            return;
        };
        let sql = match dialog.review.take(
            &dialog.editor.buffer.text(),
            !dialog.driver.is_read_only(),
            live,
        ) {
            Ok(sql) => sql,
            Err(error) => {
                dialog.result = error.to_string();
                cx.notify();
                return;
            }
        };
        let (send, cancel) = flume::bounded(1);
        dialog.cancel = Some(send);
        dialog.result = "Executing reviewed SQL…".into();
        let id = dialog.id;
        let driver = dialog.driver.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                db_wire::get_runtime().block_on(driver.execute_administration(&sql, cancel))
            }).await;
            let _ = this.update(cx, |this, cx| {
                let live = this.administration_live();
                let Some(dialog) = this.administration.as_mut().filter(|d| d.id == id) else { return; };
                dialog.review.busy = false; dialog.cancel = None;
                let success = result.is_ok();
                dialog.result = match result {
                    Ok(outcome) => {
                        let mut text = format!("Completed and committed. Rows affected/returned: {}.\n", outcome.affected);
                        if !outcome.columns.is_empty() { text.push_str(&outcome.columns.join(" | ")); text.push('\n'); }
                        for row in outcome.rows { text.push_str(&row.into_iter().map(|v| match v { Some(s) => format!("{s:?}"), None => "SQL NULL".into() }).collect::<Vec<_>>().join(" | ")); text.push('\n'); }
                        if outcome.truncated { text.push_str("Preview truncated (100 rows, 32 columns, 512 characters per cell).\n"); }
                        text
                    }
                    Err(error) => format!("Administration failed: {error:#}"),
                };
                if !live { dialog.message = "Original connection changed. Outcome belongs to the original connection; close and reopen.".into(); }
                let owner = dialog.owner.clone(); let family = dialog.family;
                if success && live {
                    this.refresh_connection_for(&owner, cx);
                    this.load_administration(family, 0, cx);
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }
    pub(super) fn administration_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.administration else {
            return;
        };
        if key.key == "escape" {
            if dialog.review.busy {
                return;
            }
            if dialog.review.sql.take().is_none() {
                self.administration = None;
            }
        } else if dialog.editable() {
            dialog.editor.handle_input_key(key, cx);
        }
        cx.notify();
    }
    pub(super) fn render_administration(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let pick: ui_editor::EditorPick = std::rc::Rc::new(move |line, col, kind, _, app| {
            let _ = weak.update(app, |this, cx| {
                if let Some(dialog) = this.administration.as_mut().filter(|d| d.editable()) {
                    dialog.editor.pick(line, col, kind);
                    cx.notify();
                }
            });
        });
        let live = self.administration_live();
        let dialog = self.administration.as_mut().unwrap();
        let ready = !dialog.loading && dialog.editable();
        let mut families = div().flex().flex_wrap().gap_1();
        for family in AdminFamily::ALL {
            let button = tool(family.label());
            families = families.child(if ready {
                button.on_click(
                    cx.listener(move |this, _, _, cx| this.load_administration(family, 0, cx)),
                )
            } else {
                button.opacity(0.5)
            });
        }
        let mut rows = div()
            .id("admin-catalog")
            .max_h(px(150.))
            .overflow_y_scroll();
        for (index, object) in dialog.objects.iter().enumerate() {
            let title = format!("{}.{} {}", object.schema, object.name, object.parent);
            let row = div().id(("admin-object", index)).px_2().py_1().child(title);
            rows = rows.child(if ready {
                row.cursor_pointer().on_click(
                    cx.listener(move |this, _, _, cx| this.administration_select(index, cx)),
                )
            } else {
                row
            });
        }
        let mut templates = div().flex().flex_wrap().gap_1();
        for (index, (label, _)) in dialog.templates.iter().enumerate() {
            let button = tool(label);
            templates = templates.child(if ready {
                button.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(dialog) = this.administration.as_mut().filter(|d| d.editable()) {
                        dialog.editor = SqlEditorView::new(
                            &dialog.templates[index].1,
                            this.theme.clone(),
                            None,
                        );
                        cx.notify();
                    }
                }))
            } else {
                button.opacity(0.5)
            });
        }
        let family = dialog.family;
        let mut actions = div().flex().gap_2();
        if ready {
            actions = actions.child(tool("Refresh").on_click(
                cx.listener(move |this, _, _, cx| this.load_administration(family, 0, cx)),
            ));
            if let Some(after) = dialog.next {
                actions = actions.child(tool("Next page").on_click(
                    cx.listener(move |this, _, _, cx| this.load_administration(family, after, cx)),
                ));
            }
        }
        if dialog.review.busy {
            actions = actions.child(tool("Cancel operation").on_click(cx.listener(
                |this, _, _, cx| {
                    if let Some(dialog) = &mut this.administration
                        && let Some(cancel) = &dialog.cancel
                    {
                        match cancel.try_send(()) {
                            Ok(()) | Err(flume::TrySendError::Full(_)) => {
                                dialog.result =
                                    "Cancellation requested; waiting for final outcome…".into()
                            }
                            Err(flume::TrySendError::Disconnected(_)) => {
                                dialog.result =
                                    "Operation already finishing; waiting for final outcome…".into()
                            }
                        }
                    }
                    cx.notify();
                },
            )));
        } else if dialog.review.sql.is_some() {
            actions = actions
                .child(tool("Go back").on_click(cx.listener(|this, _, _, cx| {
                    if let Some(d) = &mut this.administration {
                        d.review.sql = None;
                    }
                    cx.notify();
                })))
                .child(
                    tool("Confirm and execute")
                        .on_click(cx.listener(|this, _, _, cx| this.administration_execute(cx))),
                );
        } else {
            if ready && live && !dialog.driver.is_read_only() {
                actions =
                    actions
                        .child(tool("Review SQL…").on_click(
                            cx.listener(|this, _, _, cx| this.administration_review(cx)),
                        ));
            }
            actions = actions.child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                this.administration = None;
                cx.notify();
            })));
        }
        let warning = if dialog.driver.is_read_only() {
            "Read-only profile: browsing is available; execution is disabled."
        } else if dialog.review.sql.is_some() {
            "Review the exact SQL below. Confirm executes it on this profile and may change data, permissions or object definitions. Routine effects may be irreversible."
        } else {
            "Edit native SQL below. Routine arguments start as typed NULL values; replace them before review. One statement per operation."
        };
        dialog.editor.theme = theme.clone();
        let editor =
            dialog
                .editor
                .render_editor(dialog.editable(), self.input.cursor_visible, pick, window);
        let scroll = dialog.editor.scroll_handle.clone();
        let panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .w(px(1000.))
            .max_w_full()
            .h(relative(0.92))
            .min_h_0()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border_focused)
            .rounded_md()
            .child(format!("Administration · {}", dialog.owner))
            .child(families)
            .child(rows)
            .child(
                div()
                    .id("admin-details")
                    .max_h(px(90.))
                    .overflow_y_scroll()
                    .child(dialog.message.clone()),
            )
            .child(templates)
            .child(warning)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(editor),
                    )
                    .child(properties::grid::detail_scrollbar(
                        scroll.clone(),
                        true,
                        &theme,
                    )),
            )
            .child(properties::grid::detail_scrollbar(scroll, false, &theme))
            .child(
                div()
                    .id("admin-outcome")
                    .max_h(px(140.))
                    .overflow_y_scroll()
                    .child(dialog.result.clone()),
            )
            .child(actions);
        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::ReviewState;
    #[test]
    fn administration_requires_exact_review_and_live_writable_profile() {
        let mut review = ReviewState::default();
        assert!(review.take("DROP ROLE x", true, true).is_err());
        review.review("ALTER ROLE x NOLOGIN".into()).unwrap();
        assert!(review.take("DROP ROLE x", true, true).is_err());
        assert!(review.take("ALTER ROLE x NOLOGIN", false, true).is_err());
        assert!(review.take("ALTER ROLE x NOLOGIN", true, false).is_err());
        assert_eq!(
            review.take("ALTER ROLE x NOLOGIN", true, true).unwrap(),
            "ALTER ROLE x NOLOGIN"
        );
        assert!(review.busy);
        assert!(review.review("SELECT 1".into()).is_err());
    }
}

```

## Appendix C — Historical app integration patch

```diff
--- a/crates/app/src/workspace/mod.rs
+++ b/crates/app/src/workspace/mod.rs
@@ -9,6 +9,7 @@
 use ui_grid::VirtualDataGrid;

 mod about_dialog;
+mod administration;
 mod catalog_panel;
 mod column_editor;
 mod column_menu;
@@ -215,6 +216,7 @@
     /// "Choose SQL script" dialog, if open
     object_browser: Option<object_browser::ObjectBrowser>,
     session_monitor: Option<session_monitor::SessionMonitorDialog>,
+    administration: Option<administration::AdministrationDialog>,
     object_search: Option<object_search::ObjectSearch>,
     definition_search: Option<definition_search::DefinitionSearch>,
     drop_object_modal: Option<drop_object_modal::DropObjectModal>,
@@ -309,6 +311,7 @@
             allow_quit: false,
             object_browser: None,
             session_monitor: None,
+            administration: None,
             object_search: None,
             definition_search: None,
             drop_object_modal: None,
@@ -437,6 +440,7 @@
         if self.form.show_connection_modal
             || self.transaction_log.is_some()
             || self.session_monitor.is_some()
+            || self.administration.is_some()
         {
             window.focus(&self.focus_handle);
         }
@@ -658,6 +662,9 @@
         if self.session_monitor.is_some() {
             root = root.child(self.render_session_monitor(cx));
         }
+        if self.administration.is_some() {
+            root = root.child(self.render_administration(window, cx));
+        }
         if self.updates.read(cx).is_open() {
             root = root.child(self.updates.clone());
         }
--- a/crates/app/src/workspace/keyboard.rs
+++ b/crates/app/src/workspace/keyboard.rs
@@ -121,6 +121,12 @@
         }
         if self.native_backup_modal.is_some() && !self.updates.read(cx).is_open() {
             self.native_backup_key(&event.keystroke, cx);
+            return;
+        }
+        if self.administration.is_some() {
+            if self.updates.read(cx).is_open() {
+                if event.keystroke.key == "escape" { self.updates.update(cx, |updates, cx| updates.dismiss(cx)); }
+            } else { self.administration_key(&event.keystroke, cx); }
             return;
         }
         if self.navigator_filter_key(&event.keystroke, cx) {
--- a/crates/app/src/workspace/script_saves.rs
+++ b/crates/app/src/workspace/script_saves.rs
@@ -314,7 +314,8 @@
         }
     }
     pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) {
-        if self.native_backup_running()
+        if self.administration.as_ref().is_some_and(|dialog| dialog.executing())
+            || self.native_backup_running()
             || self
                 .drop_object_modal
                 .as_ref()
@@ -326,6 +327,7 @@
         {
             return;
         }
+        self.administration = None;
         self.drop_object_modal = None;
         self.native_backup_modal = None;
         self.object_browser = None;
--- a/crates/app/src/workspace/nav_context_menu.rs
+++ b/crates/app/src/workspace/nav_context_menu.rs
@@ -111,6 +111,7 @@
     Refresh,
     OpenSqlEditor,
     SessionMonitor,
+    Administration,
     NativeBackup(super::native_backup_modal::Mode),

     // Schema Actions
@@ -285,6 +286,10 @@

         match menu.target {
             NavContextTarget::Connection { .. } => {
+                if self.connections.session(&conn_id).driver.as_ref().is_some_and(|d| d.supports_administration()) {
+                    panel = panel.child(item("nav-menu-administration", "icons/database.svg", "Administration…",
+                        is_connected && !self.operations.is_running(), NavMenuAction::Administration, cx));
+                }
                 if is_connected && self.connections.session(&conn_id).kind == DbDriverKind::Postgres
                 {
                     panel = panel.child(item(
@@ -717,6 +722,7 @@
             NavMenuAction::EditConnection => self.edit_saved_connection(Some(conn_id)),
             NavMenuAction::Connect => self.connect_saved_by_id(conn_id, cx),
             NavMenuAction::SessionMonitor => self.open_session_monitor(conn_id, cx),
+            NavMenuAction::Administration => self.open_administration(conn_id, cx),
             NavMenuAction::Disconnect => self.disconnect_by_id(conn_id, cx),
             NavMenuAction::Delete => self.delete_saved_connection(conn_id, cx),
             NavMenuAction::DeleteDatabase => self.open_drop_database(conn_id, cx),

```

## Verification evidence

Feature verification on 2026-11-16:

- Sudo required interactive authentication, so approved Ubuntu development/runtime packages were extracted under `/tmp/fastdb-p201-tooling/root`. No dependency versions or application manifests changed. Missing transitive packages and a broken D-Bus development symlink were resolved by adding matching official runtime packages. Rustfmt was installed for the existing stable toolchain.
- `cargo fmt --all -- --check` passed. `cargo clippy --locked --workspace --all-targets --offline` passed; two new collapsible-if warnings were corrected, and the repeat Clippy check had no warnings.
- `cargo test --locked --workspace --offline` passed 815 reported cases across 42 suites. Environment-gated live cases may return early: this count does not claim all external providers ran.
- `cargo build --locked --workspace --offline` passed. The app launched with isolated XDG directories, remained alive for six seconds, emitted no startup diagnostics, and was terminated by the smoke-test harness. No Computer Use or manual GUI acceptance was performed.
- `FASTDB_TEST_PG_PORT=55461 FASTDB_TEST_PG_USER=postgres cargo test --locked -p db-wire --test administration_test --offline -- --nocapture` passed both tests on the owned PostgreSQL 18.6 fixture. This includes exact data/catalog read-back, native source replay, VARIADIC+OUT invocation, a multi-page sequence catalog, native permission denial, canceled in-flight writes rolled back, SQL NULL/empty distinction, result limits, read-only refusal, and unsupported SQLite.
- The stronger partition read-back assertion initially expected unnecessary quotes around a safe lowercase schema identifier. PostgreSQL `format(%I)` returned its canonical safe unquoted spelling. The exact expectation was corrected, with no production change or weakened assertion; the live suite then passed.
- The first full workspace attempt failed linking the incomplete extracted D-Bus development package (static archive with unresolved systemd symbols). Adding its matching shared runtime library fixed the environment. The full rerun passed. A live-test launch also hit an automatic approval-review timeout; the permitted single retry ran and passed.

All eight catalog queries were inspected with `EXPLAIN (ANALYZE, BUFFERS)` against 1,000 tables plus 1,000 routines. Individual execution times below are not comparative performance claims:

| Family | Execution ms | Result rows | Execution buffers |
| --- | ---: | ---: | --- |
| Partitions | 0.610 | 2 | 79 hits |
| Policies | 0.430 | 1 | 47 hits |
| Privileges | 0.338 | 51 | 140 hits |
| Roles | 0.244 | 17 | 23 hits, 1 read |
| Routines | 3.254 | 51 | 1,106 hits, 47 reads |
| Rules | 0.991 | 3 | 65 hits, 12 reads |
| Sequences | 0.367 | 1 | 48 hits |
| Triggers | 0.090 | 1 | 10 hits |

Sorts were in memory (25–28 KiB); no spill occurred. Catalog scans are present: partitions scanned 1,418 pg_class rows to find two; sequence estimates were 106 versus one actual; policy estimates were 14 versus one. The routine OID scan examined 3,288 function/procedure rows before reaching 51 user routines and materialized two namespaces. Role membership and policy-role aggregation are correlated within the one SQL request, bounded by the displayed page; index scans and memoization are visible in the plans. This fixture is evidence at that catalog scale, not a guarantee for arbitrary production catalogs.

Manual desktop checklist remains with the user: open each family, page/refresh, edit and review native source, dismiss confirmation without execution, run a routine with concrete arguments, restart a sequence, modify triggers/rules/policies, create/attach/detach a partition, create/alter/drop disposable roles and grant/revoke membership/object privileges, verify read-only and permission errors, cancel a running operation, and close/reopen after replacing the profile.

Plan review round 1 identified active-cancellation, restricted-permission, panic/timeout cleanup, variadic OUT argument, cancellation-diagnostic and integration-code gaps. The revised appendices address all six; re-review requested. Opus is unavailable through the collaboration tools, so the review uses the inherited available model.

Plan re-review corrected the restricted-role fixture to use a generated per-run password, preserving compatibility with password-authenticated fixtures. All other first-round findings were confirmed addressed; this correction is awaiting confirmation.

Execution checkpoint: inline execution began after the approved choices and two plan-review rounds. The remaining restricted-role password issue was corrected and self-verified in the final appendix. Task 1/2 deviation: the planned full-crate red builds were replaced with isolated rustc red checks while the fresh dependency cache compiled: the UI test failed on missing ReviewState and catalog inclusion failed on missing routines.sql. After adding catalog files, the same catalog inclusion check passed. These compile checks are not live behavioral verification; full owning-crate suites remain required. This deviation avoids claiming a full-crate failure that was not observed.

## Independent code review corrections

The first driver and app reviews identified four Important findings. The docs review found none.

| Finding | Correction and evidence |
| --- | --- |
| Mutable config field could bypass the original read-only policy | Use the canonical driver `is_read_only()` guard. The queued-cancel regression first returned cancellation and now returns read-only refusal before connecting. |
| Relation ACL details could be silently cropped | Fetch one lookahead character, cap the preview at 8,192 characters and add a truncation message. A native ACL with 180 test roles first lacked the marker and now asserts it and the size bound. |
| Deferred Save → Run could start SQL during administration | Reserve the shared operation slot through cleanup. The app regression checks that a released save cannot start a query before administration relinquishes the slot and that generic cancellation cannot touch the unrelated profile client. |
| A queued Quit or updater restart could exit during administration | Check departure before waiting for saves and again immediately before exit. The app regression checks that a departure decision made while idle does not authorize exit after execution starts. |

The focused PostgreSQL suite passed all three tests on 18.6. The app administration model passed three tests. The full post-fix gate passed `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --offline`, `cargo test --locked --workspace --offline` (818 reported cases across 42 suites, including environment-gated cases), and `cargo build --locked --workspace --offline`. The rebuilt app stayed alive for six seconds with isolated settings and wrote zero bytes of startup output. GUI acceptance remains with the user.

The revised native catalog query was checked with all eight EXPLAIN plans on the same 1,000-table, 1,000-routine fixture. The execution times by family were 0.602 ms (partitions), 0.379 ms (policies), 0.247 ms (privileges), 0.127 ms (roles), 2.142 ms (routines), 0.492 ms (rules), 0.249 ms (sequences), and 0.084 ms (triggers). Sorts used 25–28 KiB of memory and no spill was observed. These are individual runs.

The external Opus review was rejected by automatic approval review because it would transfer private repository content to an external service without separate authorization. The project now permits an explicitly selected Sol reviewer. The review skill's receiving-code-review sub-skill was not available locally. Findings above were checked against code and reproduced where applicable before applying fixes.

## Final verification and review

The last code changes protect SQL in the desktop subscriber, require a successful catalog read before retrying failed execution, show the named connection and endpoint, retain the existing policy definition for editing, and distinguish sending a cancellation request from confirmed rollback.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on Rust 1.100. |
| `cargo clippy --locked --workspace --all-targets --offline` | Passed. Cargo reported six unused dependency warnings in the unchanged mem-engine, platform-hal, sql-intel, and ui-editor manifests. |
| `cargo test --locked --workspace --offline` | Passed. 822 reported cases across 42 suites. This includes provider-gated cases that return early. |
| `cargo build --locked --workspace --offline` | Passed. The rebuilt `target/debug/fastdb` resolves its runtime libraries without the extracted tooling. |
| Focused native tests | The exact binaries from the workspace suite passed three driver cases and one real transport debug capture against PostgreSQL 18.7. The live suite exercises catalog families, source replay, paging, mutation read-back, denial, read-only policy, active cancellation, rollback and limits. The logging test observes a fake sentinel in an unprotected capture and verifies its absence with the app's filter enabled. |
| Startup | The rebuilt app stayed alive for six seconds with an isolated copy of the saved demo profile. The harness found no credential-load or panic marker. No Computer Use. |

The independent app and driver Sol reviews approved the final code in round three. The docs reviewer requested the historical labels and final verification record added here, and noted the SQL limit clarified in the guide. The docs reviewer approved the updated documentation. The workspace's shared operation slot and departure guards were checked at their callers. Catalog page and preview bounds avoid unbounded client accumulation. The new capability uses default trait methods for other providers. Driver errors retain user-visible messages without SQL transport debug output in the app. Standalone driver consumers and PostgreSQL server logs have their own logging configuration.

The two isolated test databases were dropped and the extracted temporary build tools were removed. The rebuilt app, Cargo artifacts, and bundled demo clients were retained. The second, redundant Cargo invocation of the focused tests was stopped while waiting for the build lock because the exact binaries already passed separately. An actual lost commit acknowledgment was not induced, and the GUI checklist remains pending with the user. The work remains uncommitted in accordance with the commit-only-on-request instruction.
