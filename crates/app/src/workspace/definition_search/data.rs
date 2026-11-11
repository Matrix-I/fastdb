use super::*;
use db_wire::objects::{DATA_PREVIEW_CHARS, DATA_SEARCH_LIMIT, DataRowPreview, DataSearchHit};
pub(super) struct ScopedHit {
    pub table: String,
    pub hit: DataSearchHit,
}

pub(super) struct TableSearch {
    pub table: String,
    pub hits: Vec<ScopedHit>,
    pub errors: Vec<String>,
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
        self.data.hits = scan.hits;
        self.data.errors = scan.errors;
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
        self.definition_search = Some(DefinitionSearch {
            owner: owner.clone(),
            driver,
            generation: session.metadata_generation,
            schema: schema.clone(),
            needle: Input::new(""),
            message:
                "Search committed values in supported scalar columns. Unsaved edits are excluded."
                    .into(),
            data: TableSearch {
                table: table.clone(),
                hits: Vec::new(),
                errors: Vec::new(),
            },
            source: None,
            cancel: None,
            task: None,
        });
        cx.notify();
    }

    pub(super) fn run_table_data_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &mut self.definition_search else {
            return;
        };
        if search.task.is_some() {
            return;
        }
        let data = &mut search.data;
        let table = data.table.clone();
        data.hits.clear();
        data.errors.clear();
        search.source = None;
        if !search.is_current(&self.connections) {
            search.message = "Connection or catalog changed. Reopen search.".into();
        } else if search.needle.text.trim().is_empty() {
            search.message = "Enter text to search for.".into();
        } else {
            let (cancel, signal) = flume::bounded(1);
            search.cancel = Some(cancel);
            let driver = search.driver.clone();
            let schema = search.schema.clone();
            let needle = search.needle.text.clone();
            search.message = format!("Searching {table:?}…");
            search.task = Some(cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn({
                        let table = table.clone();
                        let signal = signal.clone();
                        async move {
                            db_wire::get_runtime().block_on(
                                driver.search_table_data(&schema, &table, &needle, signal),
                            )
                        }
                    })
                    .await;
                let mut scan = ScopeScan::new(1);
                if !(signal.is_disconnected()
                    && result
                        .as_ref()
                        .is_err_and(|error| error.to_string() == "Data search cancelled"))
                {
                    scan.record(table, result);
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
        let Some(hit) = search.data.hits.get(index) else {
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
    use super::append_limited;

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
