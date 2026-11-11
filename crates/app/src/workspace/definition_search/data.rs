use super::*;
use db_wire::objects::{DATA_PREVIEW_CHARS, DATA_SEARCH_LIMIT, DataRowPreview, DataSearchHit};
use std::collections::BTreeSet;

const MAX_SCOPE_TABLES: usize = 32;
const VISIBLE_TABLES: usize = 100;

pub(super) enum TableScope {
    Fixed(String),
    Picker(BTreeSet<String>),
}

impl TableScope {
    fn toggle(&mut self, table: &str) -> bool {
        let Self::Picker(selected) = self else {
            return false;
        };
        if selected.remove(table) {
            return true;
        }
        if selected.len() >= MAX_SCOPE_TABLES {
            return false;
        }
        selected.insert(table.to_owned())
    }

    fn names(&self) -> Vec<String> {
        match self {
            Self::Fixed(table) => vec![table.clone()],
            Self::Picker(tables) => tables.iter().cloned().collect(),
        }
    }
}

pub(super) struct ScopedHit {
    pub table: String,
    pub hit: DataSearchHit,
}

pub(super) struct TableSearch {
    pub scope: TableScope,
    pub filter: Input,
    choices: Vec<String>,
    pub hits: Vec<ScopedHit>,
    pub errors: Vec<String>,
}

impl TableSearch {
    pub(super) fn uses_picker(&self) -> bool {
        matches!(self.scope, TableScope::Picker(_))
    }

    pub(super) fn refresh_choices(&mut self, tables: &[TableSummary]) {
        let filter = self.filter.text.to_lowercase();
        self.choices = tables
            .iter()
            .filter(|table| searchable_table(table) && table.name.to_lowercase().contains(&filter))
            .take(VISIBLE_TABLES + 1)
            .map(|table| table.name.clone())
            .collect();
    }

    pub(super) fn reset_selection(&mut self) {
        if let TableScope::Picker(selected) = &mut self.scope {
            selected.clear();
        }
    }
}

fn append_limited<T>(hits: &mut Vec<T>, incoming: impl IntoIterator<Item = T>) -> bool {
    let remaining = DATA_SEARCH_LIMIT.saturating_sub(hits.len());
    let mut incoming = incoming.into_iter();
    hits.extend(incoming.by_ref().take(remaining));
    incoming.next().is_some()
}

impl DefinitionSearch {
    fn end_data_request(
        &mut self,
        connections: &connection_state::ConnectionManager,
        failed: bool,
    ) -> bool {
        self.task = None;
        let cancelled = self.cancel.take().is_none();
        if !self.is_current(connections) {
            self.message = "Connection or catalog changed. Reopen search.".into();
            return false;
        }
        if cancelled && !failed {
            self.message = "Data search cancelled.".into();
            return false;
        }
        true
    }

    fn complete_scope_scan(
        &mut self,
        connections: &connection_state::ConnectionManager,
        scan: ScopeScan,
    ) {
        self.task = None;
        let cancelled = self.cancel.take().is_none();
        if !self.is_current(connections) {
            self.message = "Connection or catalog changed. Reopen search.".into();
            return;
        }
        let status = if cancelled {
            "Cancelled"
        } else if scan.capped {
            "Stopped at the result limit"
        } else {
            "Finished"
        };
        self.message = format!(
            "{status} · {} of {} tables searched · {} matching rows",
            scan.completed,
            scan.total,
            scan.hits.len()
        );
        if let Some(data) = &mut self.data {
            data.hits = scan.hits;
            data.errors = scan.errors;
        }
    }
}

struct ScopeScan {
    total: usize,
    completed: usize,
    capped: bool,
    hits: Vec<ScopedHit>,
    errors: Vec<String>,
}

impl ScopeScan {
    fn new(total: usize) -> Self {
        Self {
            total,
            completed: 0,
            capped: false,
            hits: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn record(&mut self, table: String, result: anyhow::Result<Option<Vec<DataSearchHit>>>) {
        match result {
            Ok(Some(hits)) => {
                self.completed += 1;
                self.capped = append_limited(
                    &mut self.hits,
                    hits.into_iter().map(|hit| ScopedHit {
                        table: table.clone(),
                        hit,
                    }),
                );
            }
            Ok(None) => self
                .errors
                .push(format!("{table:?}: Provider does not support data search")),
            Err(error) => self.errors.push(format!("{table:?}: {error}")),
        }
    }
}

fn searchable_table(table: &TableSummary) -> bool {
    table.table_type.eq_ignore_ascii_case("BASE TABLE")
        || table.table_type.eq_ignore_ascii_case("table")
}

impl Workspace {
    pub(in crate::workspace) fn open_table_data_search(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let (Some(owner), Some(schema), Some(table)) =
            (&tab.connection_id, &tab.schema_name, &tab.table_name)
        else {
            return;
        };
        let session = self.connections.active();
        if self.connections.active_connection_id.as_ref() != Some(owner)
            || !matches!(
                session.kind,
                DbDriverKind::Postgres | DbDriverKind::Sqlite | DbDriverKind::MySql
            )
        {
            return;
        }
        let Some(driver) = session.driver.clone() else {
            return;
        };
        let mut search = DefinitionSearch::new(owner.clone(), driver, session.metadata_generation);
        search.schema = Input::new(schema);
        search.data = Some(TableSearch {
            scope: TableScope::Fixed(table.clone()),
            filter: Input::new(""),
            choices: Vec::new(),
            hits: Vec::new(),
            errors: Vec::new(),
        });
        search.message =
            "Search committed values in supported scalar columns. Unsaved edits are excluded."
                .into();
        self.definition_search = Some(search);
        cx.notify();
    }

    pub(in crate::workspace) fn open_scoped_data_search(&mut self, cx: &mut Context<Self>) {
        let session = self.connections.active();
        let (Some(owner), Some(driver)) = (&self.connections.active_connection_id, &session.driver)
        else {
            return;
        };
        if !matches!(
            session.kind,
            DbDriverKind::Postgres | DbDriverKind::Sqlite | DbDriverKind::MySql
        ) {
            return;
        }
        let mut search =
            DefinitionSearch::new(owner.clone(), driver.clone(), session.metadata_generation);
        match session.kind {
            DbDriverKind::Sqlite => search.schema = Input::new("main"),
            DbDriverKind::MySql => {
                let database = self
                    .connections
                    .saved_connections
                    .iter()
                    .find(|connection| &connection.id == owner)
                    .map_or("", |connection| connection.database.as_str());
                search.schema = Input::new(database);
            }
            _ => {}
        }
        search.data = Some(TableSearch {
            scope: TableScope::Picker(BTreeSet::new()),
            filter: Input::new(""),
            choices: Vec::new(),
            hits: Vec::new(),
            errors: Vec::new(),
        });
        if let Some(data) = &mut search.data {
            let tables = session
                .schema_tree
                .get(&search.schema.text)
                .map_or(&[][..], Vec::as_slice);
            data.refresh_choices(tables);
        }
        search.field = Field::Schema;
        search.message = "Select tables from the exact schema, then enter the search text.".into();
        self.definition_search = Some(search);
        cx.notify();
    }

    fn toggle_scope_table(&mut self, schema: &str, table: &str, cx: &mut Context<Self>) {
        if let Some(search) = &mut self.definition_search
            && search.task.is_none()
            && search.is_current(&self.connections)
            && search.schema.text == schema
            && let Some(data) = &mut search.data
        {
            if data.scope.toggle(table) {
                data.hits.clear();
                data.errors.clear();
                search.source = None;
                search.message = "Press Enter to search the selected tables.".into();
            } else {
                search.message = format!("Select up to {MAX_SCOPE_TABLES} tables per search.");
            }
            cx.notify();
        }
    }

    pub(super) fn render_data_scope(
        &self,
        search: &DefinitionSearch,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let Some(data) = &search.data else {
            return div();
        };
        let TableScope::Picker(selected) = &data.scope else {
            return div();
        };
        let schema: SharedString = search.schema.text.clone().into();
        let visible = &data.choices;
        let detail = if visible.len() > VISIBLE_TABLES {
            format!(
                "{} selected · first {VISIBLE_TABLES} shown · filter to find more",
                selected.len()
            )
        } else if visible.is_empty() {
            format!(
                "{} selected · no loaded tables match this scope",
                selected.len()
            )
        } else {
            format!(
                "{} selected · select up to {MAX_SCOPE_TABLES}",
                selected.len()
            )
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .min_h_0()
            .child(self.render_definition_field(
                search,
                Field::TableFilter,
                &data.filter,
                "Filter tables…",
                cx,
            ))
            .child(detail)
            .child(
                div()
                    .id("data-scope-tables")
                    .min_h_0()
                    .max_h(px(112.))
                    .overflow_y_scroll()
                    .children(visible.iter().take(VISIBLE_TABLES).enumerate().map(
                        |(index, table)| {
                            let name = table.clone();
                            let schema = schema.clone();
                            let mark = if selected.contains(&name) {
                                "☑"
                            } else {
                                "☐"
                            };
                            div()
                                .id(("data-scope-table", index))
                                .p_1()
                                .cursor_pointer()
                                .child(format!("{mark} {name:?}"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_scope_table(&schema, &name, cx)
                                }))
                        },
                    )),
            )
    }

    pub(super) fn run_table_data_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &mut self.definition_search else {
            return;
        };
        if search.task.is_some() {
            return;
        }
        let Some(data) = &mut search.data else {
            return;
        };
        let tables = data.scope.names();
        data.hits.clear();
        data.errors.clear();
        search.source = None;
        if !search.is_current(&self.connections) {
            search.message = "Connection or catalog changed. Reopen search.".into();
        } else if tables.is_empty() {
            search.message = "Select at least one table.".into();
        } else if search.needle.text.trim().is_empty() {
            search.message = "Enter text to search for.".into();
        } else {
            let (cancel, signal) = flume::bounded(1);
            search.cancel = Some(cancel);
            let driver = search.driver.clone();
            let schema = search.schema.text.clone();
            let needle = search.needle.text.clone();
            search.task = Some(cx.spawn(async move |this, cx| {
                let mut scan = ScopeScan::new(tables.len());
                for (index, table) in tables.into_iter().enumerate() {
                    let progress = this.update(cx, |this, cx| {
                        let Some(search) = &mut this.definition_search else {
                            return false;
                        };
                        if !search.is_current(&this.connections) || search.cancel.is_none() {
                            return false;
                        }
                        search.message = format!(
                            "Searching {} of {} · {table:?} · {} matching rows",
                            index + 1,
                            scan.total,
                            scan.hits.len()
                        );
                        cx.notify();
                        true
                    });
                    if !matches!(progress, Ok(true)) {
                        break;
                    }
                    let (driver, schema, needle, request_signal, name) = (
                        driver.clone(),
                        schema.clone(),
                        needle.clone(),
                        signal.clone(),
                        table.clone(),
                    );
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            db_wire::get_runtime().block_on(driver.search_table_data(
                                &schema,
                                &name,
                                &needle,
                                request_signal,
                            ))
                        })
                        .await;
                    if signal.is_disconnected()
                        && result
                            .as_ref()
                            .is_err_and(|error| error.to_string() == "Data search cancelled")
                    {
                        break;
                    }
                    scan.record(table, result);
                    if scan.hits.len() == DATA_SEARCH_LIMIT {
                        scan.capped |= index + 1 < scan.total;
                        break;
                    }
                }
                let _ = this.update(cx, |this, cx| {
                    if let Some(search) = &mut this.definition_search {
                        search.complete_scope_scan(&this.connections, scan);
                    }
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    pub(super) fn open_data_search_match(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(search) = &mut self.definition_search else {
            return;
        };
        if search.task.is_some() || !search.is_current(&self.connections) {
            return;
        }
        let Some(hit) = search.data.as_ref().and_then(|data| data.hits.get(index)) else {
            return;
        };
        let title = format!("Matching row · {}.{}", hit.table, hit.hit.column);
        let locator = hit.hit.locator.clone();
        let driver = search.driver.clone();
        let (cancel, signal) = flume::bounded(1);
        search.cancel = Some(cancel);
        search.source = None;
        let captured = matches!(
            self.connections.active().kind,
            DbDriverKind::Sqlite | DbDriverKind::MySql
        );
        search.message = "Opening the matching row…".into();
        search.task = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                db_wire::get_runtime().block_on(driver.read_data_search_row(&locator, signal))
            }).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(search) = &mut this.definition_search
                    && search.end_data_request(&this.connections, result.is_err())
                {
                    search.message = match result {
                        Ok(Some(row)) => {
                            let omitted = row.omitted_columns;
                            search.source = Some((title.into(), format_row(row).into()));
                            let label = if captured { "Captured row preview" } else { "Read-only row preview" };
                            format!("{label} · up to {DATA_PREVIEW_CHARS} characters per value · {omitted} columns omitted")
                        }
                        Ok(None) => "Row preview is unavailable for this provider.".into(),
                        Err(error) => error.to_string(),
                    };
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

fn format_row(row: DataRowPreview) -> String {
    row.columns
        .into_iter()
        .map(|(name, value)| match value {
            Some(value) => format!("{name}: {value:?}"),
            None => format!("{name}: [SQL NULL]"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{MAX_SCOPE_TABLES, TableScope, append_limited};

    #[test]
    fn selection_is_bounded_and_uses_exact_table_names() {
        let mut scope = TableScope::Picker(Default::default());
        for index in 0..MAX_SCOPE_TABLES {
            assert!(scope.toggle(&format!("table {index}")));
        }
        assert!(!scope.toggle("other"));
        assert!(scope.toggle("table 0"));
        assert!(scope.toggle("other"));
        assert_eq!(scope.names().len(), MAX_SCOPE_TABLES);
        assert!(scope.names().contains(&"other".into()));
        assert!(!scope.names().contains(&"table 0".into()));
    }

    #[test]
    fn the_picker_filters_tables_beyond_the_visible_prefix() {
        let mut tables: Vec<_> = (0..101)
            .map(|index| db_wire::TableSummary {
                schema: "public".into(),
                name: format!("table {index}"),
                table_type: "BASE TABLE".into(),
                estimated_rows: None,
            })
            .collect();
        tables.push(db_wire::TableSummary {
            schema: "public".into(),
            name: "audit needle".into(),
            table_type: "BASE TABLE".into(),
            estimated_rows: None,
        });
        tables.push(db_wire::TableSummary {
            schema: "public".into(),
            name: "needle view".into(),
            table_type: "VIEW".into(),
            estimated_rows: None,
        });
        let mut picker = super::TableSearch {
            scope: TableScope::Picker(Default::default()),
            filter: super::Input::new(""),
            choices: Vec::new(),
            hits: Vec::new(),
            errors: Vec::new(),
        };
        picker.refresh_choices(&tables);
        assert_eq!(picker.choices.len(), super::VISIBLE_TABLES + 1);
        picker.filter = super::Input::new("NEEDLE");
        picker.refresh_choices(&tables);
        assert_eq!(picker.choices, ["audit needle"]);
    }

    #[test]
    fn a_failed_table_does_not_erase_progress_or_hide_its_error() {
        let mut scan = super::ScopeScan::new(3);
        scan.record("visible".into(), Ok(Some(Vec::new())));
        scan.record("private".into(), Err(anyhow::anyhow!("Permission denied")));
        scan.record("empty".into(), Ok(Some(Vec::new())));
        assert_eq!(scan.completed, 2);
        assert_eq!(scan.total, 3);
        assert_eq!(scan.errors, [r#""private": Permission denied"#]);
        assert!(!scan.capped);
    }

    #[test]
    fn a_scope_retains_only_the_first_fifty_hits_across_tables() {
        let mut hits = Vec::new();
        assert!(!append_limited(&mut hits, 0..40));
        assert!(append_limited(&mut hits, 40..80));
        assert_eq!(hits, (0..50).collect::<Vec<_>>());
        let mut exact = Vec::new();
        assert!(!append_limited(&mut exact, 0..50));
    }
}
