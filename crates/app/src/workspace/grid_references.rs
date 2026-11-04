use super::*;
use db_wire::{
    ForeignKey,
    filter::{Criterion, FilterOp, FilterSpec},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GridReference {
    pub name: String,
    target_schema: String,
    target_table: String,
    pairs: Vec<(String, String)>,
    direction: Direction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Direction {
    Outgoing,
    Incoming,
}

fn collect_references<'a>(
    keys: impl Iterator<Item = &'a ForeignKey>,
    schema: &str,
    table: &str,
    column: &str,
    direction: Direction,
) -> Vec<GridReference> {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for key in keys {
        let (from_schema, from_table, from_column, to_schema, to_table, to_column) = match direction
        {
            Direction::Outgoing => (
                &key.source_schema,
                &key.source_table,
                &key.source_column,
                &key.target_schema,
                &key.target_table,
                &key.target_column,
            ),
            Direction::Incoming => (
                &key.target_schema,
                &key.target_table,
                &key.target_column,
                &key.source_schema,
                &key.source_table,
                &key.source_column,
            ),
        };
        if from_schema == schema && from_table == table {
            groups
                .entry((&key.name, to_schema, to_table))
                .or_default()
                .push((from_column, to_column));
        }
    }
    groups
        .into_iter()
        .filter_map(|((name, schema, table), pairs)| {
            pairs
                .iter()
                .any(|(source, _)| *source == column)
                .then(|| GridReference {
                    name: name.clone(),
                    target_schema: schema.clone(),
                    target_table: table.clone(),
                    pairs: pairs
                        .into_iter()
                        .map(|(source, target)| (source.clone(), target.clone()))
                        .collect(),
                    direction,
                })
        })
        .collect()
}

impl GridReference {
    pub(super) fn label(&self) -> String {
        let action = match self.direction {
            Direction::Outgoing => "Open referenced row",
            Direction::Incoming => "Open referencing rows",
        };
        format!("{action} ({})", self.name)
    }
}

fn reference_filter(
    grid: &VirtualDataGrid,
    row: u64,
    reference: &GridReference,
) -> Result<FilterSpec, String> {
    let pager = grid.pager.as_ref().ok_or("Result is unavailable")?;
    if row >= grid.row_count() {
        return Err("Referenced row is no longer loaded".into());
    }
    let resolved = grid.changeset.resolve_row(row);
    let fetched = match resolved {
        ui_grid::RowRef::Fetched(row) => {
            if grid.changeset.is_deleted(row) {
                return Err("Source row is marked for deletion".into());
            }
            let (page, row) = pager.locate_row(row).ok_or("Source row is unavailable")?;
            Some((
                pager.get_page(page).map_err(|error| error.to_string())?,
                row,
            ))
        }
        ui_grid::RowRef::New(_) => None,
    };
    let mut criteria = Vec::new();
    for (source, target) in &reference.pairs {
        let col = pager
            .columns
            .iter()
            .position(|column| column.name == *source)
            .ok_or("Result does not contain every foreign key column")?;
        if matches!(
            pager.columns[col].data_type,
            DataType::Bytea | DataType::Unknown
        ) {
            return Err("Binary or unknown foreign key types cannot be looked up".into());
        }
        let value = match resolved {
            ui_grid::RowRef::New(index) => {
                grid.changeset.inserted_rows[index].values[col].as_deref()
            }
            ui_grid::RowRef::Fetched(row) => match grid.changeset.edited_value(row, col) {
                Some(value) => value,
                None => {
                    let (page, row) = fetched.as_ref().expect("fetched row has a page");
                    page.get_cell(*row, col)
                        .map(std::str::from_utf8)
                        .transpose()
                        .map_err(|_| "Foreign key value is not UTF-8")?
                }
            },
        };
        let value = value.ok_or("A NULL or unset key does not identify a related row")?;
        if value.len() > 8192 {
            return Err("Foreign key value exceeds 8 KiB".into());
        }
        criteria.push(Criterion {
            column: target.clone(),
            op: FilterOp::Equal,
            value: value.into(),
        });
    }
    Ok(FilterSpec {
        criteria,
        ..FilterSpec::default()
    })
}

impl Workspace {
    pub(super) fn grid_references(
        &self,
        index: usize,
        focus: ui_grid::GridCoord,
    ) -> Vec<GridReference> {
        let tab = &self.tabs[index];
        let (Some(schema), Some(table), Some(pager)) =
            (&tab.schema_name, &tab.table_name, &tab.grid.pager)
        else {
            return Vec::new();
        };
        let Some(column) = pager.columns.get(focus.col) else {
            return Vec::new();
        };
        let session = self.tab_session(index);
        let Some(node) = session
            .diagram
            .nodes
            .iter()
            .find(|node| &node.schema == schema && &node.table == table)
        else {
            return Vec::new();
        };
        [
            (&node.outgoing, Direction::Outgoing),
            (&node.incoming, Direction::Incoming),
        ]
        .into_iter()
        .flat_map(|(edges, direction)| {
            collect_references(
                edges.iter().map(|&index| &session.diagram.edges[index].fk),
                schema,
                table,
                &column.name,
                direction,
            )
        })
        .collect()
    }

    pub(super) fn open_grid_reference(
        &mut self,
        focus: ui_grid::GridCoord,
        reference: GridReference,
        cx: &mut Context<Self>,
    ) {
        if self.execution_busy(cx) {
            return;
        }
        let index = self.active_tab;
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if tab.schema_stale
            || self.tab_session(index).driver.is_none()
            || !self.grid_references(index, focus).contains(&reference)
        {
            return;
        }
        let Some(connection) = &tab.connection_id else {
            return;
        };
        let connection = connection.clone();
        let session = self.tab_session(index);
        let result = reference_filter(&tab.grid, focus.row, &reference).and_then(|spec| {
            let columns = session
                .table_columns
                .get(&(
                    reference.target_schema.clone(),
                    reference.target_table.clone(),
                ))
                .ok_or("Related table metadata is unavailable")?
                .iter()
                .map(|column| column.name.clone())
                .collect::<Vec<_>>();
            if matches!(session.kind, DbDriverKind::MySql | DbDriverKind::MariaDb) {
                let source = tab
                    .grid
                    .pager
                    .as_ref()
                    .ok_or("Source result is unavailable")?;
                if reference.pairs.iter().any(|(column, _)| {
                    source.columns.iter().any(|meta| {
                        meta.name == *column
                            && !matches!(meta.data_type, DataType::Text | DataType::Varchar)
                    })
                }) {
                    return Err(
                        "MySQL lookup requires text keys to avoid implicit numeric coercion".into(),
                    );
                }
            }
            let sql = spec
                .sql(session.kind, &columns)
                .map_err(|error| error.to_string())?;
            Ok((spec, sql))
        });
        let (spec, sql) = match result {
            Ok(result) => result,
            Err(error) => {
                self.tabs[index].last_error = Some(error);
                cx.notify();
                return;
            }
        };
        let cache = session.schema_cache.clone();
        let id = self.allocate_tab_id();
        let mut related = TabState::table(
            id,
            &reference.target_schema,
            &reference.target_table,
            self.theme.clone(),
            cache,
        );
        related.connection_id = Some(connection);
        related.filter_sql = sql;
        related.filter_spec = Some(spec);
        self.tabs.push(related);
        self.active_tab = self.tabs.len() - 1;
        self.tab_scroll.scroll_to_item(self.active_tab);
        self.input.is_filter_focused = false;
        self.run_active_query(cx);
    }
}

struct SelectedReference {
    source: usize,
    row: u64,
    values: Vec<(usize, String)>,
}

pub(super) struct ReferencePicker {
    source_tab: u64,
    source_pager: std::sync::Weak<db_wire::QueryResultPager>,
    source_revision: u64,
    source_driver: std::sync::Weak<dyn DatabaseDriver>,
    source_row: u64,
    source_connection: String,
    reference: GridReference,
}

impl GridReference {
    pub(super) fn can_choose(&self) -> bool {
        self.direction == Direction::Outgoing
    }
}

impl Workspace {
    pub(super) fn choose_grid_reference(
        &mut self,
        focus: ui_grid::GridCoord,
        reference: GridReference,
        cx: &mut Context<Self>,
    ) {
        if !reference.can_choose() || !self.can_edit_active_rows() {
            return;
        }
        let index = self.active_tab;
        let tab = &self.tabs[index];
        let (Some(connection), Some(pager)) = (&tab.connection_id, &tab.grid.pager) else {
            return;
        };
        if tab.schema_stale
            || tab.grid.editing_cell.is_some()
            || focus.row >= tab.grid.row_count()
            || !self.grid_references(index, focus).contains(&reference)
        {
            return;
        }
        let Some(driver) = self.tab_session(index).driver.as_ref() else {
            return;
        };
        let picker = ReferencePicker {
            source_tab: tab.id,
            source_pager: Arc::downgrade(pager),
            source_revision: tab.grid.changeset_revision(),
            source_driver: Arc::downgrade(driver),
            source_row: focus.row,
            source_connection: connection.clone(),
            reference,
        };
        let cache = self.tab_session(index).schema_cache.clone();
        let id = self.allocate_tab_id();
        let mut target = TabState::table(
            id,
            &picker.reference.target_schema,
            &picker.reference.target_table,
            self.theme.clone(),
            cache,
        );
        target.title = format!("Choose key · {}", picker.reference.target_table);
        target.connection_id = Some(picker.source_connection.clone());
        target.reference_picker = Some(picker);
        self.tabs.push(target);
        self.active_tab = self.tabs.len() - 1;
        self.tab_scroll.scroll_to_item(self.active_tab);
        self.input.is_filter_focused = false;
        self.run_active_query(cx);
    }

    pub(super) fn accept_grid_reference(&mut self, cx: &mut Context<Self>) {
        if self.execution_busy(cx) {
            return;
        }
        let index = self.active_tab;
        let result = self.selected_reference_values(index);
        let SelectedReference {
            source,
            row,
            values,
        } = match result {
            Ok(values) => values,
            Err(error) => {
                if let Some(tab) = self.tabs.get_mut(index) {
                    tab.last_error = Some(error);
                }
                cx.notify();
                return;
            }
        };
        if let Err(error) =
            super::grid_paste::stage_reference_values(&mut self.tabs[source].grid, row, values)
        {
            self.tabs[index].last_error = Some(error);
            cx.notify();
            return;
        }
        let picker_id = self.tabs[index].id;
        self.active_tab = source;
        self.close_saved_tabs(&[picker_id], cx);
        self.tab_scroll.scroll_to_item(self.active_tab);
        cx.notify();
    }

    fn selected_reference_values(&self, index: usize) -> Result<SelectedReference, String> {
        let tab = self.tabs.get(index).ok_or("Lookup tab closed")?;
        let picker = tab
            .reference_picker
            .as_ref()
            .ok_or("No active key lookup")?;
        if tab.schema_stale || tab.grid.changeset.is_dirty() || tab.grid.editing_cell.is_some() {
            return Err("Choose a saved row from the lookup result".into());
        }
        let source = self
            .tabs
            .iter()
            .position(|tab| tab.id == picker.source_tab)
            .ok_or("Source tab closed")?;
        let origin = &self.tabs[source];
        let source_pager = picker
            .source_pager
            .upgrade()
            .ok_or("Source result changed")?;
        if origin.connection_id.as_deref() != Some(&picker.source_connection)
            || !origin
                .grid
                .pager
                .as_ref()
                .is_some_and(|pager| Arc::ptr_eq(pager, &source_pager))
            || origin.grid.changeset_revision() != picker.source_revision
            || origin.schema_stale
            || origin.grid.editing_cell.is_some()
            || !self
                .tab_session(source)
                .driver
                .as_ref()
                .is_some_and(|driver| {
                    picker
                        .source_driver
                        .upgrade()
                        .is_some_and(|original| Arc::ptr_eq(driver, &original))
                })
        {
            return Err("Source result or staged edits changed. Reopen key lookup.".into());
        }
        let row = tab
            .grid
            .selection
            .focus
            .ok_or("Select a lookup row first")?
            .row;
        let pager = tab
            .grid
            .pager
            .as_ref()
            .ok_or("Lookup result is unavailable")?;
        let (page, local) = pager.locate_row(row).ok_or("Lookup row is unavailable")?;
        let page = pager.get_page(page).map_err(|error| error.to_string())?;
        let mut values = Vec::new();
        for (source, target) in &picker.reference.pairs {
            let source = source_pager
                .columns
                .iter()
                .position(|column| column.name == *source)
                .ok_or("Source key column is unavailable")?;
            let target = pager
                .columns
                .iter()
                .position(|column| column.name == *target)
                .ok_or("Lookup key column is unavailable")?;
            let bytes = page
                .get_cell(local, target)
                .ok_or("A NULL value does not identify a referenced row")?;
            if bytes.len() > 8192 || matches!(pager.columns[target].data_type, DataType::Bytea) {
                return Err("Lookup key must be text and fit within 8 KiB".into());
            }
            let value = std::str::from_utf8(bytes).map_err(|_| "Lookup key is not UTF-8")?;
            values.push((source, value.into()));
        }
        Ok(SelectedReference {
            source,
            row: picker.source_row,
            values,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Direction, collect_references, reference_filter};
    use db_wire::{ColumnMeta, DataType, ForeignKey, QueryResultPager};
    use std::sync::Arc;
    use ui_grid::VirtualDataGrid;

    #[test]
    fn composite_lookup_uses_all_qualified_pairs_and_rejects_null() {
        let keys =
            [("tenant", "tenant_id"), ("id", "customer_id")].map(|(source, target)| ForeignKey {
                name: "customer_fk".into(),
                source_schema: "public".into(),
                source_table: "orders".into(),
                source_column: source.into(),
                target_schema: "billing".into(),
                target_table: "customers".into(),
                target_column: target.into(),
            });
        let references =
            collect_references(keys.iter(), "public", "orders", "id", Direction::Outgoing);
        assert_eq!(references.len(), 1);
        let incoming = collect_references(
            keys.iter(),
            "billing",
            "customers",
            "customer_id",
            Direction::Incoming,
        );
        assert_eq!(incoming[0].target_table, "orders");
        assert_eq!(incoming[0].pairs[0], ("tenant_id".into(), "tenant".into()));
        let pager = Arc::new(
            QueryResultPager::new(
                ["id", "tenant"]
                    .map(|name| ColumnMeta::new(name, DataType::Int8))
                    .into(),
            )
            .unwrap(),
        );
        pager.append_row([Some("8"), Some("3")]).unwrap();
        pager.append_row([Some("9"), None]).unwrap();
        let grid = VirtualDataGrid::new(Some(pager), ui_core::theme::Theme::dark());
        let filter = reference_filter(&grid, 0, &references[0]).unwrap();
        assert_eq!(filter.criteria[0].value, "3");
        assert_eq!(filter.criteria[1].value, "8");
        assert!(reference_filter(&grid, 1, &references[0]).is_err());
        let sql = filter
            .sql(
                db_wire::DbDriverKind::Sqlite,
                &["tenant_id".into(), "customer_id".into()],
            )
            .unwrap();
        db_wire::get_runtime().block_on(async {
            use db_wire::{DatabaseDriver, SqliteDriver};
            let driver = SqliteDriver::new_memory();
            driver.execute_query("CREATE TABLE customers (tenant_id INT, customer_id INT, name TEXT, PRIMARY KEY (tenant_id, customer_id))").await.unwrap();
            driver.execute_query("INSERT INTO customers VALUES (3, 8, 'match'), (3, 9, 'wrong customer'), (4, 8, 'wrong tenant')").await.unwrap();
            let (rows, _, _) = driver.execute_query(&format!("SELECT name FROM customers {sql}")).await.unwrap();
            assert_eq!(rows.total_rows(), 1);
            assert_eq!(rows.get_cell(0, 0).unwrap(), b"match");
            let (plan, _, _) = driver.execute_query(&format!("EXPLAIN QUERY PLAN SELECT name FROM customers {sql}")).await.unwrap();
            let detail = String::from_utf8(plan.get_cell(0, 3).unwrap()).unwrap();
            assert!(detail.contains("USING INDEX") && detail.contains("tenant_id=? AND customer_id=?"), "{detail}");
        });
    }
}
