//! Script query plan actions and tree view.
mod file_actions;
mod files;
use super::*;
use anyhow::{Context as _, Result, ensure};
use db_wire::QueryResultPager;
use db_wire::query_plan::{PlanNode, QueryPlan};
use gpui::prelude::FluentBuilder;

const MAX_SQLITE_PLAN_NODES: u64 = 4096;
const PLAN_HEADER_ROWS: usize = 4;

pub(super) fn plan_statement(kind: DbDriverKind, statement: &str, analyze: bool) -> Result<String> {
    if kind == DbDriverKind::SqlServer {
        ensure!(!analyze, "SQL Server actual plans are unavailable");
    }
    if kind == DbDriverKind::Sqlite {
        ensure!(!analyze, "SQLite does not provide actual execution plans");
    }
    let statements =
        super::sql_script::split_for_provider(statement, kind).map_err(anyhow::Error::msg)?;
    let [statement] = statements.as_slice() else {
        anyhow::bail!("Select one {} statement to explain", kind.name());
    };
    if !matches!(
        kind,
        DbDriverKind::Sqlite
            | DbDriverKind::MySql
            | DbDriverKind::MariaDb
            | DbDriverKind::SqlServer
    ) {
        return Ok(statement.to_owned());
    }
    // SQLite can apply PRAGMAs during preparation, even under EXPLAIN.
    ensure!(
        matches!(
            plan_keyword(statement, kind).as_str(),
            "SELECT" | "WITH" | "VALUES" | "INSERT" | "UPDATE" | "DELETE" | "REPLACE"
        ),
        "Explain supports SELECT, VALUES, WITH and data changes"
    );
    Ok(statement.to_owned())
}

#[cfg(test)]
fn explain_statement(kind: DbDriverKind, statement: &str, analyze: bool) -> Result<String> {
    db_wire::query_plan::explain_sql(kind, &plan_statement(kind, statement, analyze)?, analyze)
}

fn plan_keyword(mut sql: &str, kind: DbDriverKind) -> String {
    loop {
        sql = sql.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
        if let Some(comment) = sql.strip_prefix("--") {
            sql = comment.split_once('\n').map_or("", |(_, rest)| rest);
        } else if matches!(kind, DbDriverKind::MySql | DbDriverKind::MariaDb)
            && let Some(comment) = sql.strip_prefix('#')
        {
            sql = comment.split_once('\n').map_or("", |(_, rest)| rest);
        } else if let Some(comment) = sql.strip_prefix("/*") {
            sql = comment.split_once("*/").map_or("", |(_, rest)| rest);
        } else {
            return sql
                .split(|ch: char| !ch.is_ascii_alphabetic())
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
        }
    }
}

pub(super) fn parse_plan_for(
    provider: Option<DbDriverKind>,
    pager: &QueryResultPager,
) -> Result<QueryPlan> {
    if provider == Some(DbDriverKind::SqlServer) {
        return QueryPlan::parse_sql_server(pager);
    }
    let columns = pager.columns.iter().map(|column| column.name.as_str());
    if columns.eq(["id", "parent", "notused", "detail"]) {
        ensure!(
            provider.is_none_or(|provider| provider == DbDriverKind::Sqlite),
            "Native plan provider mismatch"
        );
        return parse_sqlite_plan(pager);
    }
    ensure!(
        pager.total_rows() == 1 && pager.num_cols() == 1,
        "Expected one native plan cell"
    );
    let json = pager.get_cell(0, 0).context("EXPLAIN returned no plan")?;
    match provider {
        Some(provider) => QueryPlan::parse_for_provider(provider, &json),
        None => QueryPlan::parse(&json),
    }
}

fn parse_sqlite_plan(pager: &QueryResultPager) -> Result<QueryPlan> {
    let rows = pager.total_rows();
    ensure!(
        rows <= MAX_SQLITE_PLAN_NODES,
        "SQLite plan exceeds {MAX_SQLITE_PLAN_NODES} nodes"
    );
    let mut children = HashMap::<i64, Vec<(i64, String)>>::new();
    let mut ids = std::collections::HashSet::new();
    for row in 0..rows {
        let id = sqlite_plan_cell(pager, row, 0)?.parse()?;
        let parent = sqlite_plan_cell(pager, row, 1)?.parse()?;
        let detail = sqlite_plan_cell(pager, row, 3)?;
        ensure!(id > 0 && ids.insert(id), "Invalid SQLite plan node ID");
        children.entry(parent).or_default().push((id, detail));
    }

    let roots = children.remove(&0).unwrap_or_default();
    let mut pending: Vec<_> = roots.into_iter().rev().map(|node| (0, node)).collect();
    let mut nodes = Vec::with_capacity(rows as usize);
    while let Some((depth, (id, title))) = pending.pop() {
        let descendants = children.remove(&id).unwrap_or_default();
        nodes.push(PlanNode {
            depth,
            title,
            details: String::new(),
            cost_fraction: 0.0,
            has_children: !descendants.is_empty(),
            collapsed: false,
            metrics: Default::default(),
        });
        pending.extend(descendants.into_iter().rev().map(|node| (depth + 1, node)));
    }
    ensure!(nodes.len() as u64 == rows, "Invalid SQLite plan hierarchy");

    let summary = if nodes.is_empty() {
        "SQLite returned no query plan operators."
    } else {
        "SQLite query plan · statement not executed · no cost or runtime metrics"
    };
    Ok(QueryPlan::new(DbDriverKind::Sqlite, summary, nodes))
}

fn sqlite_plan_cell(pager: &QueryResultPager, row: u64, column: usize) -> Result<String> {
    let value = pager
        .get_cell(row, column)
        .context("Missing SQLite plan cell")?;
    String::from_utf8(value).context("Invalid SQLite plan text")
}

impl Workspace {
    fn navigate_expensive_plan(&mut self, id: u64, backwards: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
            && let Some(plan) = &mut tab.query_plan
            && let Some(selected) = plan.select_expensive(backwards)
            && let Some(visible) = plan
                .visible_indices()
                .iter()
                .position(|index| *index == selected)
        {
            tab.plan_scroll.scroll_to_item(visible + PLAN_HEADER_ROWS);
        }
        cx.notify();
    }

    pub(super) fn explain_active_statement(&mut self, analyze: bool, cx: &mut Context<Self>) {
        if self.execution_busy(cx) {
            return;
        }
        if let Some(tab) = self
            .tabs
            .get(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy)
        {
            self.save_then(vec![tab.id], AfterSave::Explain(tab.id, analyze), cx);
        }
    }

    pub(super) fn explain_saved_statement(
        &mut self,
        id: u64,
        analyze: bool,
        cx: &mut Context<Self>,
    ) {
        if self.execution_busy(cx) {
            return;
        }
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let kind = self.tab_provider(index);
        let Some(tab) = self
            .tabs
            .get_mut(index)
            .filter(|t| t.is_script() && !t.file_busy)
        else {
            return;
        };
        let statement = tab.editor.active_statement();
        let params = sql_intel::extract_parameters_with_at_sign(
            &statement,
            super::params_modal::at_sign_mode(kind),
        );
        if !params.is_empty() {
            self.open_parameter_binding_modal(id, statement, true, analyze, cx);
            return;
        }
        match plan_statement(kind, &statement, analyze) {
            Ok(sql) => {
                let Some(driver) = self.tab_session(index).driver.clone() else {
                    let owner = self.tabs[index].connection_id.clone();
                    self.edit_saved_connection(owner.as_deref());
                    cx.notify();
                    return;
                };
                self.start_pinned_plan(index, driver, sql, analyze, cx);
            }
            Err(error) => self.tabs[index].last_error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn render_query_plan(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = &self.theme;
        let tab = &self.tabs[index];
        let tab_id = tab.id;
        let plan = tab.query_plan.as_ref().expect("plan view requires a plan");
        let navigation = div()
            .flex()
            .gap_2()
            .items_center()
            .child(script_tools::tool("Save plan JSON…").on_click(cx.listener(
                move |this, _, _, cx| {
                    if let Some(index) = this.tabs.iter().position(|tab| tab.id == tab_id) {
                        this.save_plan_file(index, cx);
                    }
                },
            )))
            .child(
                script_tools::tool("Compare saved plan…").on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(index) = this.tabs.iter().position(|tab| tab.id == tab_id) {
                            this.open_plan_file(index, true, cx);
                        }
                    },
                )),
            )
            .when(plan.expensive_basis().is_some(), |row| {
                let basis = plan.expensive_basis().unwrap_or_default();
                row.child(
                    script_tools::tool("Previous expensive node").on_click(cx.listener(
                        move |this, _, _, cx| this.navigate_expensive_plan(tab_id, true, cx),
                    )),
                )
                .child(
                    script_tools::tool("Next expensive node").on_click(cx.listener(
                        move |this, _, _, cx| this.navigate_expensive_plan(tab_id, false, cx),
                    )),
                )
                .child(format!(
                    "Ranked by {basis}; inclusive operators may overlap"
                ))
            })
            .when(plan.expensive_basis().is_none(), |row| {
                row.child("No cost or runtime metrics for expensive-node navigation")
            });
        let mut tree = div().id("query-plan-tree").track_scroll(&tab.plan_scroll).flex().flex_col().flex_1().min_h_0()
            .overflow_y_scroll().p_3().gap_2().text_size(px(12.0)).text_color(theme.text_primary)
            .child(navigation)
            .child(div().text_color(theme.text_secondary).child(plan.summary.clone()))
            .child(div().text_color(theme.text_muted).child("When reported, cost bars are relative to the root and actual node times and rows are per loop."));
        let mut comparison = div().flex().flex_col().gap_2();
        if let Some(report) = &tab.plan_comparison {
            comparison = comparison
                .child(script_tools::tool("Close comparison").on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                            tab.plan_comparison = None;
                        }
                        cx.notify();
                    },
                )))
                .child(div().font_family("monospace").child(report.clone()));
        }
        tree = tree.child(comparison);
        for i in plan.visible_indices() {
            let node = &plan.nodes[i];
            let preview = plan
                .preview(i)
                .expect("every plan node has a display preview");
            let marker = if node.has_children {
                if node.collapsed { "▸" } else { "▾" }
            } else {
                "·"
            };
            tree = tree.child(
                div()
                    .id(("plan-node", i))
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap_1()
                    .ml(px(node.depth.min(32) as f32 * 18.0))
                    .p_2()
                    .border_l_2()
                    .border_color(theme.border)
                    .bg(if plan.selected_node == Some(i) {
                        theme.bg_surface_selected
                    } else {
                        theme.bg_surface
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(plan) = this
                            .tabs
                            .iter_mut()
                            .find(|t| t.id == tab_id)
                            .and_then(|t| t.query_plan.as_mut())
                            && let Some(node) = plan.nodes.get_mut(i)
                        {
                            node.collapsed = !node.collapsed;
                        }
                        cx.notify();
                    }))
                    .child(format!("{marker} {}", preview.title))
                    .child(
                        div()
                            .h(px(3.0))
                            .w(relative(node.cost_fraction))
                            .bg(theme.text_accent),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child(SharedString::new(preview.details.clone())),
                    ),
            );
        }
        tree
    }
}

#[cfg(test)]
mod tests {
    use super::{explain_statement, parse_plan_for};
    use db_wire::{DatabaseDriver, DbDriverKind, SqliteDriver, StreamEvent};

    #[test]
    fn postgres_plan_never_appends_unexplained_selected_statements() {
        for analyze in [false, true] {
            assert!(
                explain_statement(
                    DbDriverKind::Postgres,
                    "SELECT 1; DELETE FROM items",
                    analyze
                )
                .is_err()
            );
            assert!(
                explain_statement(
                    DbDriverKind::Postgres,
                    "SELECT ';'; -- trailing comment",
                    analyze
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn mysql_explain_requires_one_query_and_preserves_literals() {
        assert!(
            explain_statement(DbDriverKind::MySql, "# native comment\nSELECT 1", false).is_ok()
        );
        assert_eq!(
            explain_statement(DbDriverKind::MySql, "SELECT ';'; -- comment", false).unwrap(),
            "EXPLAIN FORMAT=JSON\nSELECT ';'"
        );
        for sql in [
            "SELECT 1; DELETE FROM items",
            "SET @value = 1",
            "EXPLAIN SELECT 1",
            "-- empty",
        ] {
            assert!(
                explain_statement(DbDriverKind::MySql, sql, false).is_err(),
                "{sql}"
            );
        }
    }

    #[test]
    fn sqlite_explain_plans_a_temp_table_on_its_pinned_session() {
        db_wire::get_runtime().block_on(async {
            let driver = SqliteDriver::new_memory();
            let session = driver.open_script_session().await.unwrap();
            let (tx, _rx) = flume::unbounded();
            session
                .execute_streaming("CREATE TEMP TABLE items (id INTEGER PRIMARY KEY)", tx)
                .await
                .unwrap();

            let sql = explain_statement(
                DbDriverKind::Sqlite,
                "SELECT id FROM items WHERE id = 42; -- trailing comment",
                false,
            )
            .unwrap();
            let (tx, rx) = flume::unbounded();
            session.execute_streaming(&sql, tx).await.unwrap();
            let pager = rx
                .drain()
                .find_map(|event| match event {
                    StreamEvent::SchemaReady(pager) => Some(pager),
                    _ => None,
                })
                .unwrap();
            let plan = parse_plan_for(None, &pager).unwrap();
            assert_eq!(plan.nodes.len(), 1);
            assert!(
                plan.nodes[0]
                    .title
                    .contains("SEARCH items USING INTEGER PRIMARY KEY")
            );
            assert_eq!(plan.nodes[0].cost_fraction, 0.0);
            assert!(plan.summary.contains("no cost"));
            session.finish().await.unwrap();
        });
    }

    #[test]
    fn sqlite_plan_preserves_tree_order_and_rejects_orphans() {
        use db_wire::{ColumnMeta, DataType, QueryResultPager};

        let pager = QueryResultPager::new(
            ["id", "parent", "notused", "detail"]
                .map(|name| ColumnMeta::new(name, DataType::Text))
                .to_vec(),
        )
        .unwrap();
        for row in [
            ["2", "0", "0", "parent"],
            ["3", "0", "0", "other root"],
            ["4", "2", "0", "child"],
        ] {
            pager.append_row(row.map(Some)).unwrap();
        }
        let mut plan = parse_plan_for(None, &pager).unwrap();
        assert_eq!(
            plan.nodes
                .iter()
                .map(|node| (node.title.as_str(), node.depth))
                .collect::<Vec<_>>(),
            [("parent", 0), ("child", 1), ("other root", 0)],
        );
        plan.nodes[0].collapsed = true;
        assert_eq!(plan.visible_indices(), [0, 2]);
        pager
            .append_row(["5", "99", "0", "orphan"].map(Some))
            .unwrap();
        assert!(
            parse_plan_for(None, &pager)
                .unwrap_err()
                .to_string()
                .contains("hierarchy")
        );
    }

    #[test]
    fn sqlite_explain_requires_one_statement_and_rejects_analyze() {
        for sql in [
            "-- comment",
            "SELECT 1; DELETE FROM items",
            "PRAGMA foreign_keys = 1",
            "/* leading comment */ -- second comment\n pRaGmA query_only = 1",
            "\u{feff}PRAGMA reverse_unordered_selects = 1",
        ] {
            assert!(explain_statement(DbDriverKind::Sqlite, sql, false).is_err());
        }
        assert!(explain_statement(DbDriverKind::Sqlite, "SELECT 1", true).is_err());
        assert!(explain_statement(DbDriverKind::Sqlite, "SELECT ';'", false).is_ok());
    }
}
