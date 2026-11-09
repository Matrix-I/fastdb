//! CSV import and table/query transfer into an existing destination table.
use super::transfer_mapping::MappingDraft;
use super::*;
use db_wire::{
    sql::SqlDialect,
    transfer::{
        csv::{CsvOptions, CsvReader, Encoding},
        import::{
            CommitPolicy, ImportControl, ImportOutcome, ImportProgress, ImportSettings,
            run_csv_import, run_table_transfer,
        },
    },
};

/// Matching destination tables are capped so the picker stays cheap to render.
pub(super) const DESTINATION_LIMIT: usize = 100;
const PREVIEW_RECORDS: usize = 10;
const DEFAULT_NULL_TOKEN: &str = "\\N";

#[derive(Default)]
pub struct TextField {
    pub text: String,
    pub caret: usize,
    pub anchor: Option<usize>,
    pub history: LineHistory,
}

impl TextField {
    fn new(text: &str) -> Self {
        Self {
            text: text.into(),
            caret: text.chars().count(),
            anchor: None,
            history: LineHistory::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImportField {
    NullToken,
    DestinationFilter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MappingPicker {
    Source(usize),
    Conversion(usize),
}

pub struct CsvSource {
    pub file_path: String,
    pub delimiter: u8,
    pub has_header: bool,
    pub encoding: Encoding,
    pub null_token: TextField,
    pub preview_rows: Vec<Vec<String>>,
}

impl CsvSource {
    pub fn options(&self) -> CsvOptions {
        CsvOptions {
            delimiter: self.delimiter,
            encoding: self.encoding,
            header: self.has_header,
            null_token: (!self.null_token.text.is_empty()).then(|| self.null_token.text.clone()),
        }
    }
}

pub struct QuerySource {
    pub driver: Arc<dyn DatabaseDriver>,
    pub sql: String,
    pub label: String,
    /// Connection, schema and table when the source is a table, used to reject self-transfer.
    pub relation: Option<(String, String, String)>,
}

pub enum ImportSource {
    Csv(CsvSource),
    Query(QuerySource),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DestinationRef {
    pub conn_id: String,
    pub schema: String,
    pub table: String,
}

pub struct Destination {
    pub relation: DestinationRef,
    pub provider: DbDriverKind,
    pub driver: Arc<dyn DatabaseDriver>,
    pub columns: Vec<ColumnMeta>,
}

pub struct ImportModal {
    pub source: ImportSource,
    pub destination: Option<Destination>,
    /// Transfer chooses its destination; CSV import is opened for one table.
    pub destination_fixed: bool,
    pub destination_filter: TextField,
    pub destination_matches: Vec<DestinationRef>,
    pub active_field: Option<ImportField>,
    pub picker: Option<MappingPicker>,
    pub mapping: MappingDraft,
    pub commit_policy: CommitPolicy,
    pub skip_invalid: bool,
    pub is_running: bool,
    pub progress: ImportProgress,
    pub status: Option<String>,
    pub error: Option<String>,
    pub control: Option<Arc<ImportControl>>,
}

impl ImportModal {
    fn new(source: ImportSource, destination: Option<Destination>) -> Self {
        let destination_fixed = destination.is_some();
        let active_field = match (&source, destination_fixed) {
            (_, false) => Some(ImportField::DestinationFilter),
            (ImportSource::Csv(_), true) => Some(ImportField::NullToken),
            (ImportSource::Query(_), true) => None,
        };
        Self {
            source,
            destination,
            destination_fixed,
            destination_filter: TextField::default(),
            destination_matches: Vec::new(),
            active_field,
            picker: None,
            mapping: MappingDraft::default(),
            commit_policy: CommitPolicy::Atomic,
            skip_invalid: false,
            is_running: false,
            progress: ImportProgress::default(),
            status: None,
            error: None,
            control: None,
        }
    }

    pub fn csv(destination: Destination) -> Self {
        Self::new(
            ImportSource::Csv(CsvSource {
                file_path: String::new(),
                delimiter: b',',
                has_header: true,
                encoding: Encoding::Utf8,
                null_token: TextField::new(DEFAULT_NULL_TOKEN),
                preview_rows: Vec::new(),
            }),
            Some(destination),
        )
    }

    pub fn transfer(source: QuerySource, source_columns: Vec<String>) -> Self {
        let mut modal = Self::new(ImportSource::Query(source), None);
        modal.mapping = MappingDraft::auto(source_columns, &[]);
        modal
    }

    pub fn is_transfer(&self) -> bool {
        matches!(self.source, ImportSource::Query(_))
    }

    pub fn set_destination(&mut self, destination: Destination) {
        self.mapping = MappingDraft::auto(self.mapping.sources.clone(), &destination.columns);
        self.destination = Some(destination);
        self.picker = None;
        self.error = None;
        self.status = None;
    }

    /// Re-reads the CSV preview with the current options; mapping survives unchanged headers.
    pub fn reload_preview(&mut self) {
        let ImportSource::Csv(csv) = &mut self.source else {
            return;
        };
        if csv.file_path.is_empty() {
            return;
        }
        self.error = None;
        self.picker = None;
        match read_preview(&csv.file_path, csv.options()) {
            Ok((headers, rows)) => {
                csv.preview_rows = rows;
                let targets = self.destination.as_ref().map_or(&[][..], |d| &d.columns);
                self.mapping = self.mapping.with_sources(headers, targets);
                self.status = Some(format!(
                    "Loaded preview with {} columns, {} sample rows",
                    self.mapping.sources.len(),
                    csv.preview_rows.len()
                ));
            }
            Err(error) => {
                // Keep the last good mapping so fixing the option restores manual choices.
                csv.preview_rows.clear();
                self.status = None;
                self.error = Some(error);
            }
        }
    }

    /// Async results apply only to the run that started them, never to a reopened modal.
    pub fn owns(&self, control: &Arc<ImportControl>) -> bool {
        self.control
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, control))
    }

    /// Escape/close stop a run but keep the modal until its outcome is reported.
    /// Returns whether the modal may be dismissed.
    pub fn dismiss(&mut self) -> bool {
        if self.picker.take().is_some() {
            return false;
        }
        match &self.control {
            Some(control) => {
                control.cancel();
                self.status = Some("Cancelling…".into());
                false
            }
            None => true,
        }
    }

    pub fn load_file(&mut self, path: String) {
        if let ImportSource::Csv(csv) = &mut self.source {
            csv.file_path = path;
        }
        self.reload_preview();
    }

    pub fn active_text(&mut self) -> Option<&mut TextField> {
        match self.active_field? {
            ImportField::NullToken => match &mut self.source {
                ImportSource::Csv(csv) => Some(&mut csv.null_token),
                ImportSource::Query(_) => None,
            },
            ImportField::DestinationFilter => {
                (!self.destination_fixed).then_some(&mut self.destination_filter)
            }
        }
    }

    fn self_transfer(&self) -> bool {
        let (ImportSource::Query(source), Some(dest)) = (&self.source, &self.destination) else {
            return false;
        };
        source.relation.as_ref().is_some_and(|(c, s, t)| {
            *c == dest.relation.conn_id && *s == dest.relation.schema && *t == dest.relation.table
        })
    }
}

fn read_preview(
    path: &str,
    options: CsvOptions,
) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;
    let mut reader =
        CsvReader::new(file, options).map_err(|e| format!("Failed to parse CSV: {e}"))?;
    let headers = reader.headers().map(<[String]>::to_vec);
    let mut rows = Vec::new();
    while rows.len() < PREVIEW_RECORDS {
        match reader.next_record() {
            Ok(Some(row)) => rows.push(
                row.into_iter()
                    .map(|v| v.unwrap_or_else(|| "[NULL]".into()))
                    .collect(),
            ),
            Ok(None) => break,
            Err(e) => return Err(format!("Error reading CSV preview: {e}")),
        }
    }
    let headers = match headers {
        Some(h) => h,
        None => (1..=rows.first().map_or(0, Vec::len))
            .map(|i| format!("Col {i}"))
            .collect(),
    };
    Ok((headers, rows))
}

/// Destination tables on connected, writable drivers whose name contains `query`.
pub(super) fn destination_matches(
    sessions: &HashMap<String, ConnectionSession>,
    query: &str,
) -> Vec<DestinationRef> {
    let query = query.trim().to_lowercase();
    let mut conn_ids: Vec<_> = sessions
        .iter()
        .filter(|(_, s)| {
            s.driver
                .as_ref()
                .is_some_and(|d| d.supports_transfer() && !d.is_read_only())
        })
        .map(|(id, _)| id.as_str())
        .collect();
    conn_ids.sort_unstable();
    let mut matches = Vec::new();
    for conn_id in conn_ids {
        let session = &sessions[conn_id];
        for t in &session.tables {
            if t.table_type.to_ascii_uppercase().contains("VIEW")
                || !session
                    .table_columns
                    .contains_key(&(t.schema.clone(), t.name.clone()))
            {
                continue;
            }
            let qualified = format!("{conn_id} {}.{}", t.schema, t.name).to_lowercase();
            if query.is_empty() || qualified.contains(&query) {
                matches.push(DestinationRef {
                    conn_id: conn_id.into(),
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                });
                if matches.len() == DESTINATION_LIMIT {
                    return matches;
                }
            }
        }
    }
    matches
}

/// The PostgreSQL relation name is passed as a quoted literal, never as raw SQL.
fn postgres_oid_query(schema: &str, table: &str) -> String {
    let dialect = SqlDialect(DbDriverKind::Postgres);
    let relation = dialect.qualified_table(schema, table);
    format!(
        "SELECT {}::regclass::oid::text",
        dialect.literal(Some(&relation))
    )
}

/// Returns the user-facing outcome text and whether it reports a failure.
pub(super) fn outcome_message(transfer: bool, outcome: &ImportOutcome) -> (String, bool) {
    let verb = if transfer { "Transfer" } else { "Import" };
    let p = outcome.progress;
    let committed = if outcome.commit_unknown {
        "commit outcome unknown".to_string()
    } else {
        format!("{} row(s) committed", p.committed)
    };
    let mut message = if let Some(error) = &outcome.error {
        if outcome.cancelled {
            format!("{verb} cancelled; {committed}.")
        } else {
            format!("{verb} failed: {error:#}; {committed}.")
        }
    } else if outcome.cancelled {
        format!("{verb} cancelled; {committed}.")
    } else {
        format!(
            "{verb} finished: {committed}, {} processed, {} rejected.",
            p.processed, p.rejected
        )
    };
    if let Some(cleanup) = &outcome.cleanup_error {
        message.push_str(&format!(" Cleanup error: {cleanup:#}"));
    }
    let failed = (outcome.error.is_some() && !outcome.cancelled)
        || outcome.commit_unknown
        || outcome.cleanup_error.is_some();
    (message, failed)
}

impl Workspace {
    fn destination_for(&self, relation: DestinationRef) -> Option<Destination> {
        let session = self.connections.sessions.get(&relation.conn_id)?;
        let driver = session.driver.clone()?;
        let columns = session
            .table_columns
            .get(&(relation.schema.clone(), relation.table.clone()))
            .cloned()
            .unwrap_or_default();
        Some(Destination {
            provider: session.kind,
            driver,
            columns,
            relation,
        })
    }

    pub(super) fn open_import_modal(
        &mut self,
        conn_id: &str,
        schema: &str,
        table: &str,
        cx: &mut Context<Self>,
    ) {
        let relation = DestinationRef {
            conn_id: conn_id.into(),
            schema: schema.into(),
            table: table.into(),
        };
        if let Some(destination) = self.destination_for(relation) {
            self.import_modal = Some(ImportModal::csv(destination));
            cx.notify();
        }
    }

    pub(super) fn open_transfer_modal(
        &mut self,
        source: QuerySource,
        columns: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let mut modal = ImportModal::transfer(source, columns);
        modal.destination_matches = destination_matches(&self.connections.sessions, "");
        self.import_modal = Some(modal);
        cx.notify();
    }

    /// Whether a table's transfer source can be built from loaded metadata.
    pub(super) fn can_transfer_table(&self, conn_id: &str, schema: &str, table: &str) -> bool {
        self.connections.sessions.get(conn_id).is_some_and(|s| {
            s.driver.is_some()
                && s.table_columns
                    .contains_key(&(schema.to_string(), table.to_string()))
        })
    }

    /// Transfers a table (all rows, explicit column list) into another table.
    pub(super) fn open_table_transfer(
        &mut self,
        conn_id: &str,
        schema: &str,
        table: &str,
        filter: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        const NOT_READY: &str =
            "Transfer needs a connected source with loaded columns; refresh the connection";
        let session = self.connections.sessions.get(conn_id).ok_or(NOT_READY)?;
        let driver = session.driver.clone().ok_or(NOT_READY)?;
        let columns = session
            .table_columns
            .get(&(schema.to_string(), table.to_string()))
            .ok_or(NOT_READY)?;
        let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
        let dialect = SqlDialect(session.kind);
        let target = super::export::sql_target(dialect, Some(schema), Some(table));
        let sql = super::export::table_source_sql(&target, Some((dialect, &names)), filter);
        let source = QuerySource {
            driver,
            sql,
            label: format!("{conn_id} {schema}.{table}"),
            relation: Some((conn_id.into(), schema.into(), table.into())),
        };
        self.open_transfer_modal(source, names, cx);
        Ok(())
    }

    pub(super) fn refresh_destination_matches(&mut self) {
        let sessions = &self.connections.sessions;
        if let Some(modal) = &mut self.import_modal {
            modal.destination_matches =
                destination_matches(sessions, &modal.destination_filter.text);
        }
    }

    pub(super) fn select_destination(&mut self, relation: DestinationRef, cx: &mut Context<Self>) {
        let Some(destination) = self.destination_for(relation) else {
            return;
        };
        if let Some(modal) = &mut self.import_modal
            && !modal.is_running
        {
            modal.set_destination(destination);
            modal.active_field = None;
            cx.notify();
        }
    }

    pub(super) fn import_modal_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.import_modal else {
            return;
        };
        if key.key == "escape" {
            if modal.dismiss() {
                self.import_modal = None;
            }
            cx.notify();
            return;
        }
        if modal.is_running {
            return;
        }
        let field = modal.active_field;
        let Some(text) = modal.active_text() else {
            return;
        };
        let input = LineInput {
            text: &mut text.text,
            caret: &mut text.caret,
            anchor: &mut text.anchor,
            digits_only: false,
            history: &mut text.history,
        };
        if !handle_line_key(input, key, cx) {
            return;
        }
        match field {
            Some(ImportField::NullToken) => modal.reload_preview(),
            Some(ImportField::DestinationFilter) => self.refresh_destination_matches(),
            None => {}
        }
        cx.notify();
    }

    pub(super) fn execute_import(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = &mut self.import_modal else {
            return;
        };
        if modal.is_running {
            return;
        }
        let Some(destination) = &modal.destination else {
            modal.error = Some("Choose a destination table".into());
            cx.notify();
            return;
        };
        if modal.self_transfer() {
            modal.error = Some("Source and destination are the same table".into());
            cx.notify();
            return;
        }
        let mapping = match modal.mapping.build() {
            Ok(mapping) => mapping,
            Err(e) => {
                modal.error = Some(format!("Mapping error: {e:#}"));
                cx.notify();
                return;
            }
        };
        let (csv, input) = match &modal.source {
            ImportSource::Csv(csv) if csv.file_path.is_empty() => {
                modal.error = Some("Choose a CSV file".into());
                cx.notify();
                return;
            }
            ImportSource::Csv(csv) => match std::fs::File::open(&csv.file_path) {
                Ok(file) => (csv.options(), Ok(file)),
                Err(e) => {
                    modal.error = Some(format!("Cannot open source file: {e}"));
                    cx.notify();
                    return;
                }
            },
            ImportSource::Query(q) => (
                CsvOptions::default(),
                Err((q.driver.clone(), q.sql.clone())),
            ),
        };
        let transfer = modal.is_transfer();
        let relation = destination.relation.clone();
        let provider = destination.provider;
        let dest_driver = destination.driver.clone();
        let commit = modal.commit_policy;
        let skip_invalid_values = modal.skip_invalid;

        let control = Arc::new(ImportControl::default());
        modal.control = Some(control.clone());
        modal.is_running = true;
        modal.picker = None;
        modal.active_field = None;
        modal.progress = ImportProgress::default();
        modal.status = Some(if transfer {
            "Transfer in progress…".into()
        } else {
            "Import in progress…".into()
        });
        modal.error = None;
        cx.notify();

        let mut progress_rx = control.progress();
        let run = control.clone();
        cx.spawn(async move |this, cx| {
            let mut expected_object_id = None;
            if provider == DbDriverKind::Postgres {
                let query = postgres_oid_query(&relation.schema, &relation.table);
                match dest_driver.execute_query(&query).await {
                    Ok((res, _, _)) => {
                        expected_object_id =
                            res.get_cell(0, 0).and_then(|c| String::from_utf8(c).ok());
                    }
                    Err(e) => {
                        let _ = this.update(cx, |this, cx| {
                            if let Some(m) = &mut this.import_modal
                                && m.owns(&run)
                            {
                                m.is_running = false;
                                m.control = None;
                                m.status = None;
                                m.error = Some(format!("Cannot resolve destination table: {e:#}"));
                                cx.notify();
                            }
                        });
                        return;
                    }
                }
            }
            let settings = ImportSettings {
                schema: relation.schema,
                table: relation.table,
                expected_object_id,
                csv,
                mapping,
                commit,
                skip_invalid_values,
            };
            let progress_task = cx.spawn({
                let this = this.clone();
                let run = run.clone();
                async move |cx| {
                    while progress_rx.changed().await.is_ok() {
                        let p = *progress_rx.borrow();
                        let _ = this.update(cx, |this, cx| {
                            if let Some(m) = &mut this.import_modal
                                && m.owns(&run)
                                && !run.is_cancelled()
                            {
                                m.progress = p;
                                m.status = Some(format!(
                                    "Processed: {}, Committed: {}, Rejected: {}",
                                    p.processed, p.committed, p.rejected
                                ));
                                cx.notify();
                            }
                        });
                    }
                }
            });
            let outcome = match input {
                Ok(file) => run_csv_import(dest_driver, file, settings, control).await,
                Err((source_driver, sql)) => {
                    run_table_transfer(source_driver, sql, dest_driver, settings, control).await
                }
            };
            drop(progress_task);
            let _ = this.update(cx, |this, cx| {
                if let Some(m) = &mut this.import_modal
                    && m.owns(&run)
                {
                    let (message, failed) = outcome_message(transfer, &outcome);
                    m.is_running = false;
                    m.control = None;
                    m.progress = outcome.progress;
                    if failed {
                        m.status = None;
                        m.error = Some(message);
                    } else {
                        m.error = None;
                        m.status = Some(message);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionSession, CsvSource, DESTINATION_LIMIT, Destination, DestinationRef,
        ImportControl, ImportModal, ImportSource, MappingPicker, QuerySource, TextField,
        destination_matches, outcome_message, postgres_oid_query,
    };
    use db_wire::{
        ColumnMeta, DbDriverKind, TableSummary,
        transfer::{csv::Encoding, import::ImportOutcome},
    };
    use mem_engine::DataType;
    use std::{collections::HashMap, io::Write, sync::Arc};

    fn destination() -> Destination {
        Destination {
            relation: DestinationRef {
                conn_id: "c".into(),
                schema: "main".into(),
                table: "users".into(),
            },
            provider: DbDriverKind::Sqlite,
            driver: Arc::new(db_wire::sqlite::SqliteDriver::new_memory()),
            columns: vec![
                ColumnMeta::new("id", DataType::Int4),
                ColumnMeta::new("name", DataType::Text),
            ],
        }
    }

    fn csv_file(dir: &tempfile::TempDir, bytes: &[u8]) -> String {
        let path = dir.path().join("users.csv");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        path.display().to_string()
    }

    fn csv(modal: &mut ImportModal) -> &mut CsvSource {
        match &mut modal.source {
            ImportSource::Csv(csv) => csv,
            ImportSource::Query(_) => unreachable!(),
        }
    }

    #[test]
    fn preview_loads_headers_and_auto_mapping_and_reports_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = csv_file(&dir, b"id,NAME,score\n1,alice,95.5\n2,\\N,80\n");
        let mut modal = ImportModal::csv(destination());
        modal.load_file(path);
        assert!(modal.error.is_none());
        assert_eq!(modal.mapping.sources, vec!["id", "NAME", "score"]);
        assert_eq!(csv(&mut modal).preview_rows[1], vec!["2", "[NULL]", "80"]);
        let sources: Vec<_> = modal.mapping.rows.iter().map(|r| r.source).collect();
        assert_eq!(sources, vec![Some(0), Some(1)]);

        modal.load_file("/nonexistent/path/never_there.csv".into());
        assert!(modal.error.is_some());
        assert!(csv(&mut modal).preview_rows.is_empty());
        assert_eq!(modal.mapping.sources, vec!["id", "NAME", "score"]);
    }

    #[test]
    fn null_token_and_encoding_change_the_preview() {
        let dir = tempfile::tempdir().unwrap();
        let path = csv_file(&dir, b"id,name\n1,NULL\n2,\\N\n");
        let mut modal = ImportModal::csv(destination());
        csv(&mut modal).null_token = TextField::new("NULL");
        modal.load_file(path.clone());
        assert_eq!(csv(&mut modal).preview_rows[0], vec!["1", "[NULL]"]);
        assert_eq!(csv(&mut modal).preview_rows[1], vec!["2", "\\N"]);
        assert_eq!(
            csv(&mut modal).options().null_token.as_deref(),
            Some("NULL")
        );

        csv(&mut modal).null_token = TextField::new("");
        modal.reload_preview();
        assert_eq!(csv(&mut modal).options().null_token, None);
        assert_eq!(csv(&mut modal).preview_rows[0], vec!["1", "NULL"]);

        let utf16: Vec<u8> = "id,name\n1,é\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let path16 = csv_file(&dir, &utf16);
        csv(&mut modal).encoding = Encoding::Utf16Le;
        modal.load_file(path16);
        assert!(modal.error.is_none(), "{:?}", modal.error);
        assert_eq!(csv(&mut modal).preview_rows[0], vec!["1", "é"]);
        csv(&mut modal).encoding = Encoding::Utf8;
        modal.reload_preview();
        assert!(
            modal.error.is_some(),
            "UTF-16 bytes are not valid UTF-8 CSV"
        );
    }

    #[test]
    fn headerless_files_need_manual_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let path = csv_file(&dir, b"1,alice\n");
        let mut modal = ImportModal::csv(destination());
        csv(&mut modal).has_header = false;
        modal.load_file(path);
        assert_eq!(modal.mapping.sources, vec!["Col 1", "Col 2"]);
        assert!(modal.mapping.build().is_err());
        modal.mapping.set_source(1, Some(1));
        assert_eq!(modal.mapping.build().unwrap().columns().len(), 1);
    }

    #[test]
    fn async_results_only_reach_the_run_that_started_them() {
        let mut modal = ImportModal::csv(destination());
        assert!(modal.dismiss(), "an idle modal closes");
        let run = Arc::new(ImportControl::default());
        modal.control = Some(run.clone());
        assert!(modal.owns(&run));
        assert!(!modal.owns(&Arc::new(ImportControl::default())));
        assert!(!modal.dismiss(), "a running modal stays open");
        assert!(run.is_cancelled());
        modal.picker = Some(MappingPicker::Source(0));
        assert!(!modal.dismiss());
        assert!(modal.picker.is_none());
    }

    #[test]
    fn preview_errors_keep_manual_mapping() {
        let dir = tempfile::tempdir().unwrap();
        // Odd byte length cannot decode as UTF-16.
        let path = csv_file(&dir, b"a,b\n1,xy\n");
        let mut modal = ImportModal::csv(destination());
        modal.load_file(path);
        modal.mapping.set_source(1, Some(1));
        csv(&mut modal).encoding = Encoding::Utf16Be;
        modal.reload_preview();
        assert!(modal.error.is_some());
        csv(&mut modal).encoding = Encoding::Utf8;
        modal.reload_preview();
        assert!(modal.error.is_none());
        assert_eq!(modal.mapping.rows[1].source, Some(1));
    }

    #[test]
    fn oid_query_quotes_relation_as_literal() {
        assert_eq!(
            postgres_oid_query("a'b", "t\"x"),
            r#"SELECT '"a''b"."t""x"'::regclass::oid::text"#
        );
    }

    #[test]
    fn self_transfer_is_detected() {
        let source = QuerySource {
            driver: Arc::new(db_wire::sqlite::SqliteDriver::new_memory()),
            sql: "SELECT 1".into(),
            label: "x".into(),
            relation: Some(("c".into(), "main".into(), "users".into())),
        };
        let mut modal = ImportModal::transfer(source, vec!["id".into()]);
        assert!(!modal.self_transfer());
        modal.set_destination(destination());
        assert!(modal.self_transfer());
        assert_eq!(modal.mapping.rows[0].source, Some(0));
    }

    #[test]
    fn destination_matches_filter_writable_tables_and_cap() {
        let mut session = ConnectionSession {
            driver: Some(Arc::new(db_wire::sqlite::SqliteDriver::new_memory())),
            ..Default::default()
        };
        let mut columns = HashMap::new();
        for i in 0..150 {
            session.tables.push(TableSummary {
                schema: "main".into(),
                name: format!("t{i}"),
                table_type: "table".into(),
                estimated_rows: None,
            });
            columns.insert(("main".into(), format!("t{i}")), vec![]);
        }
        session.tables.push(TableSummary {
            schema: "main".into(),
            name: "v_report".into(),
            table_type: "VIEW".into(),
            estimated_rows: None,
        });
        columns.insert(("main".into(), "v_report".into()), vec![]);
        session.table_columns = Arc::new(columns);
        let mut sessions = HashMap::new();
        sessions.insert("c".to_string(), session);
        sessions.insert("offline".to_string(), ConnectionSession::default());

        assert_eq!(destination_matches(&sessions, "").len(), DESTINATION_LIMIT);
        let hits = destination_matches(&sessions, "MAIN.T14");
        let names: Vec<_> = hits.iter().map(|h| h.table.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "t14", "t140", "t141", "t142", "t143", "t144", "t145", "t146", "t147", "t148",
                "t149"
            ]
        );
        assert!(destination_matches(&sessions, "report").is_empty());
    }

    #[test]
    fn outcome_messages_distinguish_success_cancel_failure_and_unknown_commit() {
        let mut outcome = ImportOutcome::default();
        outcome.progress.processed = 3;
        outcome.progress.committed = 3;
        let (msg, failed) = outcome_message(false, &outcome);
        assert!(!failed);
        assert_eq!(
            msg,
            "Import finished: 3 row(s) committed, 3 processed, 0 rejected."
        );

        outcome.cancelled = true;
        outcome.error = Some(anyhow::anyhow!("Import cancelled"));
        outcome.progress.committed = 1;
        let (msg, failed) = outcome_message(true, &outcome);
        assert!(!failed);
        assert_eq!(msg, "Transfer cancelled; 1 row(s) committed.");

        outcome.cancelled = false;
        outcome.error = Some(anyhow::anyhow!("constraint"));
        outcome.commit_unknown = true;
        let (msg, failed) = outcome_message(false, &outcome);
        assert!(failed);
        assert_eq!(msg, "Import failed: constraint; commit outcome unknown.");
    }
}
