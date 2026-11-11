# SQLite Definition Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: development-superpowers:subagent-driven-development or executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Add SQLite definition search via the existing Rust driver trait and GPUI modal. Use rusqlite on a dedicated read-only worker. Current worktree is on main, with no commit authorized.

## Confirmed Requirements

- P2-04 targets PostgreSQL, MySQL and SQLite.
- Search object definitions and table data across a selected scope with bounded results, progress/cancel and navigation to matches.
- The user approved additions to search result kinds and read-only search APIs.

## Global Constraints

- Keep existing PostgreSQL behavior. GUI acceptance is manual. No commit or push without a request.
- Use bound values, deterministic resource cleanup, and no new dependencies.
- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and build and run the app.

## Technical Defaults

- SQLite searches the persistent `main` catalog of the saved connection. TEMP and ATTACH aliases belong to individual SQL sessions.
- Use SQLite native ASCII case folding and bound literal substring matching. Preserve the existing 50 display limit, 51st hit and 64 KiB preview limit.
- Show native table, view, index and trigger SQL. Use catalog name and relation for display identity.

## Risks & Rollback

- An SQLite lock can delay catalog reads. Limit the busy wait and use the progress hook on an isolated connection.
- Revert the SQLite trait implementation and UI capability wiring to roll back. Search does not change persisted state.

## Task 1 — Native search and modal entry

Files: `crates/db-wire/src/sqlite/{mod.rs,definition_search.rs}`, `crates/db-wire/src/objects.rs`, `crates/db-wire/tests/catalog_test.rs`, `crates/app/src/workspace/{definition_search.rs,navigator.rs}`.

Interface: existing `DatabaseDriver::search_definitions` with SQLite implementation and additive `DefinitionKind::Table`.

- [ ] Replace the SQLite unsupported test with this regression. Run `cargo test -p db-wire --test catalog_test --locked sqlite_definition_search`. Expect failure before the enum and driver are implemented.

```rust
#[tokio::test]
async fn sqlite_definition_search_matches_native_source_and_respects_scope() {
    use db_wire::objects::DefinitionKind;
    let driver = SqliteDriver::new_memory();
    for sql in [
        "CREATE TABLE items (value TEXT DEFAULT 'Needle_%')",
        "CREATE TABLE decoy (value TEXT DEFAULT 'needleAB')",
        "CREATE VIEW example AS SELECT 'Needle_%' AS value",
        "CREATE INDEX ix ON items(value) WHERE value = 'Needle_%'",
        "CREATE TRIGGER tr AFTER INSERT ON items WHEN NEW.value = 'Needle_%' BEGIN SELECT 1; END",
    ] {
        driver.execute_query(sql).await.unwrap();
    }
    let (cancel, signal) = flume::bounded(1);
    let hits = driver.search_definitions("main", "needle_%", signal.clone()).await.unwrap().unwrap();
    assert_eq!(hits.iter().map(|hit| hit.kind).collect::<Vec<_>>(), [
        DefinitionKind::Table, DefinitionKind::View, DefinitionKind::Index, DefinitionKind::Trigger,
    ]);
    assert_eq!(hits[2].object.qualifier, "items");
    assert!(hits.iter().all(|hit| hit.source.as_deref().unwrap().contains("Needle_%")));
    assert!(driver.search_definitions("temp", "needle", signal.clone()).await.is_err());
    for index in 0..52 {
        driver.execute_query(&format!("CREATE VIEW bulk_{index} AS SELECT 'bulk marker'")).await.unwrap();
    }
    let capped = driver.search_definitions("main", "bulk marker", signal.clone()).await.unwrap().unwrap();
    assert_eq!(capped.len(), db_wire::objects::DEFINITION_SEARCH_LIMIT + 1);
    let wide = "x".repeat(db_wire::objects::DEFINITION_SOURCE_LIMIT as usize);
    driver.execute_query(&format!("CREATE VIEW wide AS SELECT 'wide marker {wide}'")).await.unwrap();
    let wide = driver.search_definitions("main", "wide marker", signal.clone()).await.unwrap().unwrap();
    assert_eq!(wide.len(), 1);
    assert!(wide[0].source.is_none());
    cancel.send(()).unwrap();
    assert!(driver.search_definitions("main", "needle", signal).await.unwrap_err().to_string().contains("cancelled"));
}
```

- [ ] Add `DefinitionKind::Table` and `mod definition_search;` in SQLite. Override the existing trait method with:

```rust
async fn search_definitions(
    &self, schema: &str, needle: &str, cancel: flume::Receiver<()>,
) -> Result<Option<Vec<crate::objects::DefinitionHit>>> {
    definition_search::search(self.session_database.clone(), self.conn.clone(), schema, needle, cancel).await
}
```

- [ ] Add `crates/db-wire/src/sqlite/definition_search.rs`.

```rust
use super::*;
use crate::objects::{
    DEFINITION_INPUT_LIMIT, DEFINITION_SEARCH_LIMIT, DEFINITION_SOURCE_LIMIT, DefinitionHit,
    DefinitionKind, ObjectSummary,
};
use std::time::Duration;

const SEARCH_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(1);
const CANCELLATION_CHECK_OPS: i32 = 1_000;

pub(super) async fn search(
    database: PathBuf,
    owned: Arc<Mutex<Connection>>,
    schema: &str,
    needle: &str,
    cancel: flume::Receiver<()>,
) -> Result<Option<Vec<DefinitionHit>>> {
    anyhow::ensure!(
        schema == "main",
        "SQLite definition search requires the main schema"
    );
    anyhow::ensure!(
        !needle.trim().is_empty()
            && needle.len() <= DEFINITION_INPUT_LIMIT
            && !needle.contains('\0'),
        "Enter search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes without NUL characters"
    );
    let needle = needle.to_owned();
    crate::get_runtime()
        .spawn_blocking(move || {
            let cancelled = move || cancel.is_disconnected() || cancel.try_recv().is_ok();
            anyhow::ensure!(!cancelled(), "Definition search cancelled");
            let conn = Connection::open_with_flags(
                read_only_path(database),
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            if conn.path() == Some("") {
                // Private memory URI profiles keep their catalog on the original handle.
                let owned = owned.try_lock_for(LOCK_TIMEOUT)
                    .context("SQLite connection is busy")?;
                return find_interruptible(&owned, &needle, cancelled).map(Some);
            }
            conn.busy_timeout(LOCK_TIMEOUT)?;
            conn.authorizer(Some(authorize_read_only));
            find_interruptible(&conn, &needle, cancelled).map(Some)
        })
        .await
        .context("SQLite definition search worker failed")?
}

fn find_interruptible(
    conn: &Connection,
    needle: &str,
    cancelled: impl Fn() -> bool + Send + 'static,
) -> Result<Vec<DefinitionHit>> {
    anyhow::ensure!(!cancelled(), "Definition search cancelled");
    let started = Instant::now();
    conn.progress_handler(
        CANCELLATION_CHECK_OPS,
        Some(move || cancelled() || started.elapsed() >= SEARCH_TIMEOUT),
    );
    let result = find(conn, needle);
    conn.progress_handler(0, None::<fn() -> bool>);
    if let Err(error) = &result
        && error.downcast_ref::<rusqlite::Error>().and_then(rusqlite::Error::sqlite_error_code)
            == Some(rusqlite::ErrorCode::OperationInterrupted)
    {
        if started.elapsed() >= SEARCH_TIMEOUT {
            anyhow::bail!("Definition search timed out");
        }
        anyhow::bail!("Definition search cancelled");
    }
    result
}

fn read_only_path(database: PathBuf) -> PathBuf {
    let Some((name, options)) = database
        .to_str()
        .filter(|path| path.starts_with("file:"))
        .and_then(|path| path.split_once('?'))
    else {
        return database;
    };
    // SQLite decodes option names and values before enforcing read-only flags.
    let (options, fragment) = options.split_once('#').unwrap_or((options, ""));
    let options = options
        .split('&')
        .map(|option| {
            if let Some((key, value)) = option.split_once('=')
                && uri_option_equals(key, b"mode")
                && (uri_option_equals(value, b"rw") || uri_option_equals(value, b"rwc"))
            {
                "mode=ro"
            } else {
                option
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    if fragment.is_empty() {
        PathBuf::from(format!("{name}?{options}"))
    } else {
        PathBuf::from(format!("{name}?{options}#{fragment}"))
    }
}

fn uri_option_equals(option: &str, expected: &[u8]) -> bool {
    let mut bytes = option.as_bytes().iter().copied();
    for &expected in expected {
        let actual = match bytes.next() {
            Some(b'%') => {
                let Some(high) = bytes.next().and_then(|byte| char::from(byte).to_digit(16)) else {
                    return false;
                };
                let Some(low) = bytes.next().and_then(|byte| char::from(byte).to_digit(16)) else {
                    return false;
                };
                (high * 16 + low) as u8
            }
            Some(byte) => byte,
            None => return false,
        };
        if actual != expected {
            return false;
        }
    }
    bytes.next().is_none()
}

fn find(conn: &Connection, needle: &str) -> Result<Vec<DefinitionHit>> {
    let mut query = conn.prepare(
        "SELECT rowid, type, name, tbl_name,
                CASE WHEN length(sql) <= ?2 THEN sql END
         FROM main.sqlite_schema
         WHERE type IN ('table', 'view', 'index', 'trigger')
           AND sql IS NOT NULL AND instr(lower(sql), lower(?1)) > 0
         ORDER BY rowid LIMIT ?3",
    )?;
    let mut rows = query.query(rusqlite::params![
        needle,
        DEFINITION_SOURCE_LIMIT,
        (DEFINITION_SEARCH_LIMIT + 1) as i64
    ])?;
    let mut hits = Vec::new();
    while let Some(row) = rows.next()? {
        let (kind, related) = match row.get_ref(1)?.as_str()? {
            "table" => (DefinitionKind::Table, false),
            "view" => (DefinitionKind::View, false),
            "index" => (DefinitionKind::Index, true),
            "trigger" => (DefinitionKind::Trigger, true),
            _ => anyhow::bail!("Unexpected SQLite definition kind"),
        };
        let source: Option<String> = row.get(4)?;
        hits.push(DefinitionHit {
            kind,
            object: ObjectSummary {
                id: u32::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
                schema: "main".into(),
                name: row.get(2)?,
                qualifier: if related { row.get(3)? } else { String::new() },
            },
            source: source.filter(|text| text.len() <= DEFINITION_SOURCE_LIMIT as usize),
        });
    }
    Ok(hits)
}
```

- [ ] Add `DefinitionKind::Table => "Table"` to `hit_title`. Update modal creation:

```rust
    pub(super) fn open_definition_search(&mut self, cx: &mut Context<Self>) {
        let session = self.connections.active();
        let (Some(owner), Some(driver)) = (&self.connections.active_connection_id, &session.driver)
        else {
            return;
        };
        if !matches!(session.kind, DbDriverKind::Postgres | DbDriverKind::Sqlite) {
            return;
        }
        let mut search =
            DefinitionSearch::new(owner.clone(), driver.clone(), session.metadata_generation);
        if session.kind == DbDriverKind::Sqlite {
            search.schema = Input::new("main");
        }
        self.definition_search = Some(search);
        cx.notify();
    }

```

- [ ] Render the definition and data entries with separate provider checks. Use provider-neutral progress and connection fallback labels.

```rust
            .when(
                matches!(
                    self.connections.active().kind,
                    DbDriverKind::Postgres | DbDriverKind::Sqlite
                ) && self.connections.active().driver.is_some(),
                |panel| {
                    panel.child(
                        script_tools::tool("Search definitions…").on_click(
                            cx.listener(|this, _, _, cx| this.open_definition_search(cx)),
                        ),
                    )
                },
            )
            .when(
                self.connections.active().kind == DbDriverKind::Postgres
                    && self.connections.active().driver.is_some(),
                |panel| {
                    panel.child(
                        script_tools::tool("Search table data…").on_click(
                            cx.listener(|this, _, _, cx| this.open_scoped_data_search(cx)),
                        ),
                    )
                },
            )
```

- [ ] Run the regression and verify SQLite native source, exact scope and cancellation.
- [ ] Ready to commit the files above — `feat(db-wire): search SQLite object definitions`.

## Task 2 — Verification & Docs

- [ ] Update `docs/guide/navigator-and-tables.md`, `docs/guide/not-built-yet.md`, `docs/PROVIDERS.md` and `docs/ROADMAP.md` with provider scope and manual acceptance.
- [ ] Review full diff and untracked files, then run formatter, workspace Clippy, workspace tests and app build. Inspect SQLite EXPLAIN QUERY PLAN on a catalog fixture.
- [ ] Launch the built app for a startup smoke test. Report manual modal verification as pending. Obtain independent code review, apply findings and repeat final gate if edited.
- [ ] Ready to commit docs — `docs: describe SQLite definition search`.


## Verification evidence

The SQLite 3.54 query plan on 1,000 view definitions scanned
`main.sqlite_schema` without a sort and returned 51 rows at the bound limit.
The file-URI regression initially failed with `access mode not allowed: rwc`.
The implementation reopens standard writable URI modes as read-only.
