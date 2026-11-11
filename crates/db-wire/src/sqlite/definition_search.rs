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
    run_catalog_read(database, owned, cancel, "Definition search", move |conn| {
        find(conn, &needle).map(Some)
    })
    .await
}

pub(super) async fn run_catalog_read<T: Send + 'static>(
    database: PathBuf,
    owned: Arc<Mutex<Connection>>,
    cancel: flume::Receiver<()>,
    label: &'static str,
    query: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
) -> Result<T> {
    crate::get_runtime()
        .spawn_blocking(move || {
            let cancelled = move || cancel.is_disconnected() || cancel.try_recv().is_ok();
            anyhow::ensure!(!cancelled(), "{label} cancelled");
            let conn = Connection::open_with_flags(
                read_only_path(database),
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            if conn.path() == Some("") {
                // Private memory URI profiles keep their catalog on the original handle.
                let owned = owned
                    .try_lock_for(LOCK_TIMEOUT)
                    .context("SQLite connection is busy")?;
                return find_interruptible(&owned, cancelled, label, query);
            }
            conn.busy_timeout(LOCK_TIMEOUT)?;
            conn.authorizer(Some(authorize_read_only));
            find_interruptible(&conn, cancelled, label, query)
        })
        .await
        .context("SQLite catalog search worker failed")?
}

fn find_interruptible<T>(
    conn: &Connection,
    cancelled: impl Fn() -> bool + Send + 'static,
    label: &str,
    query: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    anyhow::ensure!(!cancelled(), "{label} cancelled");
    let started = Instant::now();
    conn.progress_handler(
        CANCELLATION_CHECK_OPS,
        Some(move || cancelled() || started.elapsed() >= SEARCH_TIMEOUT),
    );
    let result = query(conn);
    conn.progress_handler(0, None::<fn() -> bool>);
    if let Err(error) = &result
        && error
            .downcast_ref::<rusqlite::Error>()
            .and_then(rusqlite::Error::sqlite_error_code)
            == Some(rusqlite::ErrorCode::OperationInterrupted)
    {
        if started.elapsed() >= SEARCH_TIMEOUT {
            anyhow::bail!("{label} timed out");
        }
        anyhow::bail!("{label} cancelled");
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
