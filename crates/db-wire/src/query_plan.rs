//! Native EXPLAIN generation and presentation model.
mod mysql;
mod mysql_tree;
mod sql_server;
use crate::DbDriverKind;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::sync::Arc;

const MAX_PLAN_BYTES: usize = 16 * 1024 * 1024;
const MAX_PLAN_NODES: usize = 4096;
const MAX_TITLE_CHARS: usize = 512;
const MAX_DETAIL_CHARS: usize = 2048;

#[derive(Debug)]
pub struct PlanPreview {
    pub title: Arc<str>,
    pub details: Arc<str>,
}

fn preview(text: &str, limit: usize) -> Arc<str> {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}… [see raw result]", &text[..end]).into(),
        None => text.into(),
    }
}

/// Optimizer estimates and observed per-loop values are separate measurements.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanMetrics {
    pub estimated_cost: Option<f64>,
    pub estimated_rows: Option<f64>,
    pub actual_rows_per_loop: Option<f64>,
    pub actual_loops: Option<f64>,
    pub actual_first_ms_per_loop: Option<f64>,
    pub actual_total_ms_per_loop: Option<f64>,
}

impl PlanMetrics {
    pub fn inclusive_work_ms(&self) -> Option<f64> {
        let total = self.actual_total_ms_per_loop? * self.actual_loops?;
        total.is_finite().then_some(total)
    }
}

fn nonnegative(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0)
}

pub fn explain_sql(kind: DbDriverKind, statement: &str, analyze: bool) -> Result<String> {
    if statement.trim().is_empty() {
        bail!("Place the caret in a statement to explain");
    }
    if kind == DbDriverKind::Sqlite {
        anyhow::ensure!(!analyze, "SQLite does not provide actual execution plans");
        return Ok(format!("EXPLAIN QUERY PLAN\n{statement}"));
    }
    if kind == DbDriverKind::MySql {
        let prefix = if analyze {
            "EXPLAIN ANALYZE FORMAT=TREE"
        } else {
            "EXPLAIN FORMAT=JSON"
        };
        return Ok(format!("{prefix}\n{statement}"));
    }
    if kind == DbDriverKind::MariaDb {
        let prefix = if analyze {
            "ANALYZE FORMAT=JSON"
        } else {
            "EXPLAIN FORMAT=JSON"
        };
        return Ok(format!("{prefix}\n{statement}"));
    }
    if kind != DbDriverKind::Postgres {
        bail!("This provider does not support the requested visual plan mode yet");
    }
    let options = if analyze {
        "ANALYZE, BUFFERS, FORMAT JSON"
    } else {
        "FORMAT JSON"
    };
    Ok(format!("EXPLAIN ({options})\n{statement}"))
}

#[derive(Debug)]
pub struct PlanNode {
    pub depth: usize,
    pub title: String,
    pub details: String,
    pub cost_fraction: f32,
    pub has_children: bool,
    pub collapsed: bool,
    pub metrics: PlanMetrics,
}

#[derive(Debug)]
pub struct QueryPlan {
    pub provider: DbDriverKind,
    pub summary: String,
    pub nodes: Vec<PlanNode>,
    pub selected_node: Option<usize>,
    ranking_basis: Option<&'static str>,
    ranking: Vec<usize>,
    previews: Vec<PlanPreview>,
}

impl QueryPlan {
    pub fn parse_sql_server(pager: &crate::QueryResultPager) -> Result<Self> {
        sql_server::parse(pager)
    }

    pub fn parse_for_provider(provider: DbDriverKind, bytes: &[u8]) -> Result<Self> {
        if provider == DbDriverKind::MariaDb {
            anyhow::ensure!(bytes.len() <= MAX_PLAN_BYTES, "Query plan exceeds 16 MiB");
            let value: Value =
                serde_json::from_slice(bytes).context("Invalid MariaDB EXPLAIN JSON")?;
            return mysql::parse_for(&value, provider);
        }
        let plan = Self::parse(bytes)?;
        anyhow::ensure!(
            plan.provider == provider,
            "Native plan does not match the requested provider"
        );
        Ok(plan)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        anyhow::ensure!(bytes.len() <= MAX_PLAN_BYTES, "Query plan exceeds 16 MiB");
        if let Ok(text) = std::str::from_utf8(bytes)
            && text.trim_start().starts_with("->")
        {
            return mysql_tree::parse(text);
        }
        let value: Value = serde_json::from_slice(bytes).context("Invalid EXPLAIN JSON")?;
        if value.get("query_block").is_some() {
            return mysql::parse(&value);
        }
        let document = value
            .as_array()
            .filter(|a| a.len() == 1)
            .and_then(|a| a.first())
            .context("Expected one EXPLAIN plan")?;
        let root = document.get("Plan").context("Missing EXPLAIN Plan")?;
        let mut nodes = Vec::new();
        Self::visit(
            root,
            0,
            root["Total Cost"].as_f64().unwrap_or(0.0),
            &mut nodes,
        )?;
        let summary = ["Planning Time", "Execution Time"]
            .into_iter()
            .filter_map(|key| document[key].as_f64().map(|v| format!("{key}: {v:.3} ms")))
            .collect::<Vec<_>>()
            .join(" · ");
        Ok(Self::new(
            DbDriverKind::Postgres,
            if summary.is_empty() {
                "Estimated plan (statement not executed)".into()
            } else {
                summary
            },
            nodes,
        ))
    }

    fn visit(value: &Value, depth: usize, root_cost: f64, nodes: &mut Vec<PlanNode>) -> Result<()> {
        anyhow::ensure!(
            nodes.len() < MAX_PLAN_NODES,
            "Query plan exceeds {MAX_PLAN_NODES} nodes"
        );
        let operator = value["Node Type"]
            .as_str()
            .context("Missing plan node type")?;
        let mut title = operator.to_string();
        for key in ["Relation Name", "Index Name", "Join Type", "Subplan Name"] {
            if let Some(s) = value[key].as_str() {
                title.push_str(&format!(" · {s}"));
            }
        }
        let mut details = Vec::new();
        for key in [
            "Startup Cost",
            "Total Cost",
            "Plan Rows",
            "Actual Startup Time",
            "Actual Total Time",
            "Actual Rows",
            "Actual Loops",
            "Rows Removed by Filter",
            "Shared Hit Blocks",
            "Shared Read Blocks",
            "Temp Read Blocks",
            "Temp Written Blocks",
            "Filter",
            "Index Cond",
            "Hash Cond",
            "Merge Cond",
            "Join Filter",
            "Sort Key",
        ] {
            if let Some(v) = value.get(key) {
                let unit = if key.ends_with("Time") { " ms" } else { "" };
                let text = v
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string());
                details.push(format!("{key}: {text}{unit}"));
            }
        }
        let children = match value.get("Plans") {
            None => &[][..],
            Some(v) => v.as_array().context("Invalid plan children")?.as_slice(),
        };
        nodes.push(PlanNode {
            depth,
            title,
            details: details.join(" · "),
            cost_fraction: if root_cost > 0.0 {
                (value["Total Cost"].as_f64().unwrap_or(0.0) / root_cost).clamp(0.0, 1.0) as f32
            } else {
                0.0
            },
            has_children: !children.is_empty(),
            collapsed: false,
            metrics: PlanMetrics {
                estimated_cost: nonnegative(&value["Total Cost"]),
                estimated_rows: nonnegative(&value["Plan Rows"]),
                actual_rows_per_loop: nonnegative(&value["Actual Rows"]),
                actual_loops: nonnegative(&value["Actual Loops"]),
                actual_first_ms_per_loop: nonnegative(&value["Actual Startup Time"]),
                actual_total_ms_per_loop: nonnegative(&value["Actual Total Time"]),
            },
        });
        for child in children {
            Self::visit(child, depth + 1, root_cost, nodes)?;
        }
        Ok(())
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        let mut hidden_below = None;
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, node)| {
                if hidden_below.is_some_and(|depth| node.depth > depth) {
                    return None;
                }
                hidden_below = node.collapsed.then_some(node.depth);
                Some(i)
            })
            .collect()
    }

    pub fn new(provider: DbDriverKind, summary: impl Into<String>, nodes: Vec<PlanNode>) -> Self {
        let actual = nodes
            .iter()
            .any(|node| node.metrics.inclusive_work_ms().is_some());
        let mut ranked: Vec<_> = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let metric = if actual {
                    node.metrics.inclusive_work_ms()
                } else {
                    node.metrics.estimated_cost
                }?;
                Some((index, metric))
            })
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let ranking_basis = if ranked.is_empty() {
            None
        } else if actual {
            Some("inclusive time × loops (ms)")
        } else {
            Some("native cumulative estimated cost")
        };
        Self {
            provider,
            summary: summary.into(),
            previews: nodes
                .iter()
                .map(|node| PlanPreview {
                    title: preview(&node.title, MAX_TITLE_CHARS),
                    details: preview(&node.details, MAX_DETAIL_CHARS),
                })
                .collect(),
            nodes,
            selected_node: None,
            ranking_basis,
            ranking: ranked.into_iter().map(|(index, _)| index).collect(),
        }
    }

    pub fn expensive_basis(&self) -> Option<&'static str> {
        self.ranking_basis
    }

    pub fn preview(&self, index: usize) -> Option<&PlanPreview> {
        self.previews.get(index)
    }

    pub fn expensive_indices(&self) -> &[usize] {
        &self.ranking
    }

    pub fn select_expensive(&mut self, backwards: bool) -> Option<usize> {
        let ranked = &self.ranking;
        if ranked.is_empty() {
            return None;
        }
        let position = self
            .selected_node
            .and_then(|selected| ranked.iter().position(|index| *index == selected));
        let next = match (position, backwards) {
            (Some(position), false) => (position + 1) % ranked.len(),
            (Some(position), true) => (position + ranked.len() - 1) % ranked.len(),
            (None, false) => 0,
            (None, true) => ranked.len() - 1,
        };
        let selected = ranked[next];
        self.selected_node = Some(selected);
        let mut depth = self.nodes[selected].depth;
        for node in self.nodes[..selected].iter_mut().rev() {
            if node.depth < depth {
                node.collapsed = false;
                depth = node.depth;
            }
        }
        Some(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mariadb_no_table_analyze_never_claims_the_statement_was_unexecuted() {
        let plan = QueryPlan::parse_for_provider(DbDriverKind::MariaDb, br#"{"query_optimization":{"r_total_time_ms":0.25},"query_block":{"select_id":1,"table":{"message":"No tables used"}}}"#).unwrap();
        assert!(plan.summary.contains("Actual"), "{}", plan.summary);
        assert!(!plan.summary.contains("not executed"));
        assert!(
            plan.nodes
                .iter()
                .all(|node| node.metrics.actual_loops.is_none())
        );
    }
    #[test]
    fn native_text_is_preserved_while_display_previews_are_bounded() {
        let relation = "名".repeat(600);
        let filter = "文".repeat(5000);
        let json = serde_json::json!([{"Plan":{"Node Type":"Scan", "Relation Name":relation, "Filter":filter}}]);
        let plan = QueryPlan::parse(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(plan.nodes[0].title.ends_with(&relation));
        assert!(plan.nodes[0].details.ends_with(&filter));
        let preview = plan.preview(0).unwrap();
        assert!(preview.title.contains("[see raw result]"));
        assert!(preview.details.contains("[see raw result]"));
        assert!(preview.title.chars().count() < 550);
        assert!(preview.details.chars().count() < 2100);
    }
    #[test]
    fn mariadb_native_metrics_keep_provider_identity_and_missing_values() {
        let plan = QueryPlan::parse_for_provider(DbDriverKind::MariaDb, br#"{"query_block":{"select_id":1,"r_loops":1,"r_total_time_ms":8,"nested_loop":[{"table":{"table_name":"items","rows":3,"r_loops":2,"r_rows":2,"r_table_time_ms":5,"r_other_time_ms":1}}]}}"#).unwrap();
        assert_eq!(plan.provider, DbDriverKind::MariaDb);
        assert!(plan.summary.contains("Actual"));
        assert_eq!(plan.nodes[0].metrics.actual_total_ms_per_loop, Some(8.0));
        assert_eq!(plan.nodes[2].metrics.estimated_rows, Some(3.0));
        assert_eq!(plan.nodes[2].metrics.actual_rows_per_loop, Some(2.0));
        assert_eq!(plan.nodes[2].metrics.actual_total_ms_per_loop, None);
        assert!(plan.nodes[2].details.contains("r_table_time_ms"));
        assert!(
            plan.nodes
                .iter()
                .all(|node| node.metrics.estimated_cost.is_none())
        );
        let estimate = QueryPlan::parse_for_provider(
            DbDriverKind::MariaDb,
            br#"{"query_block":{"select_id":1,"table":{"table_name":"items","rows":3}}}"#,
        )
        .unwrap();
        assert!(estimate.summary.contains("Estimated"));
        assert_eq!(
            explain_sql(DbDriverKind::MariaDb, "SELECT 1", true).unwrap(),
            "ANALYZE FORMAT=JSON\nSELECT 1"
        );
        assert!(
            QueryPlan::parse_for_provider(
                DbDriverKind::Sqlite,
                br#"[{"Plan":{"Node Type":"Result"}}]"#
            )
            .is_err()
        );
    }
    #[test]
    fn mysql_actual_tree_preserves_estimates_and_observed_per_loop_metrics() {
        assert_eq!(
            explain_sql(DbDriverKind::MySql, "SELECT 1", true).unwrap(),
            "EXPLAIN ANALYZE FORMAT=TREE\nSELECT 1"
        );
        let plan = QueryPlan::parse(b"-> Filter: (items.n > 2)  (cost=1.2 rows=2) (actual time=209e-6..0.2 rows=2 loops=3)\n    -> Table scan on items  (cost=0..1 rows=3) (actual time=0.01..0.02 rows=3 loops=3)\n").unwrap();
        assert_eq!(plan.provider, DbDriverKind::MySql);
        assert_eq!(plan.nodes.len(), 2);
        assert_eq!(plan.nodes[1].depth, 1);
        assert_eq!(plan.nodes[1].metrics.estimated_cost, Some(1.0));
        assert_eq!(
            plan.nodes[0].metrics.actual_first_ms_per_loop,
            Some(0.000209)
        );
        assert_eq!(plan.nodes[0].metrics.actual_total_ms_per_loop, Some(0.2));
        assert_eq!(plan.nodes[0].metrics.actual_loops, Some(3.0));
        assert!(plan.summary.contains("Actual"));
        for input in [
            "-> Missing metrics",
            "    -> Root (never executed)",
            "-> Root (actual time=bad..1 rows=1 loops=1)",
            "-> Root (never executed)\n        -> Invalid depth (never executed)",
        ] {
            assert!(QueryPlan::parse(input.as_bytes()).is_err(), "{input}");
        }
        let skipped = QueryPlan::parse(b"-> Root (cost=1 rows=0) (never executed)").unwrap();
        assert_eq!(skipped.nodes[0].metrics.actual_total_ms_per_loop, None);
    }
    #[test]
    fn expensive_navigation_uses_observed_work_and_reveals_ancestors() {
        let mut plan = QueryPlan::parse(br#"[{"Plan":{"Node Type":"Root","Total Cost":100,"Plan Rows":9,"Actual Total Time":4,"Actual Loops":1,"Plans":[{"Node Type":"Branch","Total Cost":80,"Plans":[{"Node Type":"Repeated","Total Cost":1,"Actual Total Time":2,"Actual Loops":3,"Actual Rows":5},{"Node Type":"Unmeasured","Total Cost":90}]}]}}]"#).unwrap();
        assert_eq!(plan.nodes[0].metrics.estimated_rows, Some(9.0));
        assert_eq!(plan.nodes[2].metrics.actual_rows_per_loop, Some(5.0));
        assert_eq!(plan.nodes[2].metrics.inclusive_work_ms(), Some(6.0));
        assert_eq!(plan.expensive_indices(), [2, 0]);
        plan.nodes[0].collapsed = true;
        plan.nodes[1].collapsed = true;
        assert_eq!(plan.visible_indices(), [0]);
        assert_eq!(plan.select_expensive(false), Some(2));
        assert_eq!(plan.visible_indices(), [0, 1, 2, 3]);
        assert_eq!(plan.select_expensive(false), Some(0));
        assert_eq!(plan.select_expensive(false), Some(2));
        assert_eq!(plan.select_expensive(true), Some(0));
        assert!(plan.expensive_basis().unwrap().contains("time"));
    }

    #[test]
    fn estimated_navigation_keeps_stable_ties_and_missing_metrics() {
        let mut plan = QueryPlan::parse(br#"[{"Plan":{"Node Type":"Root","Total Cost":10,"Plans":[{"Node Type":"First","Total Cost":3},{"Node Type":"Second","Total Cost":3},{"Node Type":"Missing"},{"Node Type":"Invalid","Total Cost":-2}]}}]"#).unwrap();
        assert_eq!(plan.expensive_indices(), [0, 1, 2]);
        assert_eq!(plan.select_expensive(true), Some(2));
        assert!(plan.expensive_basis().unwrap().contains("estimated"));
        let mut empty = QueryPlan::parse(br#"[{"Plan":{"Node Type":"Result"}}]"#).unwrap();
        assert_eq!(empty.select_expensive(false), None);
        assert_eq!(empty.expensive_basis(), None);
        assert_eq!(
            PlanMetrics {
                actual_loops: Some(f64::MAX),
                actual_total_ms_per_loop: Some(2.0),
                ..Default::default()
            }
            .inclusive_work_ms(),
            None
        );
    }
    #[test]
    fn mysql_estimated_sql_and_native_plan() {
        assert_eq!(
            explain_sql(DbDriverKind::MySql, "SELECT id FROM items", false).unwrap(),
            "EXPLAIN FORMAT=JSON\nSELECT id FROM items"
        );
        let plan = QueryPlan::parse(br#"{"query_block":{"select_id":1,"cost_info":{"query_cost":"12.50"},"nested_loop":[{"table":{"table_name":"a","access_type":"ALL","rows_examined_per_scan":100,"cost_info":{"prefix_cost":"10.00"}}},{"table":{"table_name":"b","access_type":"ref","key":"idx_a","rows_examined_per_scan":2,"attached_condition":"b.a_id = a.id","cost_info":{"prefix_cost":"12.50"}}}]}}"#).unwrap();
        assert_eq!(plan.nodes.len(), 4);
        assert_eq!(plan.nodes[0].title, "Query block 1");
        assert_eq!(plan.nodes[1].title, "Nested loop");
        assert_eq!(plan.nodes[2].depth, 2);
        assert!(plan.nodes[2].title.contains("a"));
        assert_eq!(plan.nodes[2].cost_fraction, 0.8);
        assert!(plan.nodes[3].details.contains("idx_a"));
        assert!(plan.nodes[3].details.contains("rows_examined_per_scan: 2"));
        assert!(plan.summary.contains("Estimated"));
        assert!(!plan.summary.contains("Execution Time"));
    }

    #[test]
    fn mysql_subqueries_and_missing_costs_remain_native() {
        let plan = QueryPlan::parse(br#"{"query_block":{"select_id":1,"table":{"table_name":"derived","materialized_from_subquery":{"using_temporary_table":true,"query_block":{"select_id":2,"table":{"table_name":"source","access_type":"index"}}}}}}"#).unwrap();
        assert_eq!(plan.nodes.last().unwrap().depth, 4);
        assert!(plan.nodes.last().unwrap().title.contains("source"));
        assert!(plan.nodes.iter().all(|node| node.cost_fraction == 0.0));
        for input in [
            r#"{"query_block":null}"#,
            r#"{"query_block":[]}"#,
            r#"{"query_block":{"nested_loop":"bad"}}"#,
            r#"{"query_block":{"table":null}}"#,
            r#"{"unrelated":{}}"#,
        ] {
            assert!(QueryPlan::parse(input.as_bytes()).is_err(), "{input}");
        }
    }
    #[test]
    fn mysql_windows_keep_individual_native_identity_and_order() {
        let plan = QueryPlan::parse(br#"{"query_block":{"select_id":1,"windowing":{"windows":[{"name":"first","definition_position":1,"functions":["row_number"]},{"name":"second","definition_position":2,"functions":["sum"]}]}}}"#).unwrap();
        let windows: Vec<_> = plan
            .nodes
            .iter()
            .filter(|node| node.title == "Window")
            .collect();
        assert_eq!(windows.len(), 2);
        assert!(windows[0].details.contains("name: first"));
        assert!(!windows[0].details.contains("second"));
        assert!(windows[1].details.contains("name: second"));
        assert_eq!(windows[0].depth, 3);
    }
    #[test]
    fn mysql_plan_node_and_input_limits_are_enforced() {
        let tables: Vec<_> = (0..4096)
            .map(|_| serde_json::json!({"table": {"table_name": "item"}}))
            .collect();
        let oversized = serde_json::to_vec(&serde_json::json!({
            "query_block": {"select_id": 1, "nested_loop": tables}
        }))
        .unwrap();
        assert!(
            QueryPlan::parse(&oversized)
                .unwrap_err()
                .to_string()
                .contains("4096")
        );
        let oversized = vec![b' '; 16 * 1024 * 1024 + 1];
        assert!(
            QueryPlan::parse(&oversized)
                .unwrap_err()
                .to_string()
                .contains("16 MiB")
        );
    }
    #[test]
    fn postgres_operator_count_is_bounded_before_view_construction() {
        let children: Vec<_> = (0..4096)
            .map(|_| serde_json::json!({"Node Type":"Result"}))
            .collect();
        let raw = serde_json::to_vec(
            &serde_json::json!([{"Plan":{"Node Type":"Append","Plans":children}}]),
        )
        .unwrap();
        assert!(QueryPlan::parse(&raw).is_err());
    }
    #[test]
    fn sql_preserves_statement_without_paging() {
        assert_eq!(
            explain_sql(DbDriverKind::Postgres, "SELECT 1; -- end", false).unwrap(),
            "EXPLAIN (FORMAT JSON)\nSELECT 1; -- end"
        );
        assert!(
            explain_sql(DbDriverKind::Postgres, "UPDATE t SET a=1", true)
                .unwrap()
                .starts_with("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)")
        );
        assert_eq!(
            explain_sql(DbDriverKind::Sqlite, "SELECT 1", false).unwrap(),
            "EXPLAIN QUERY PLAN\nSELECT 1"
        );
        assert!(explain_sql(DbDriverKind::Postgres, "  ", false).is_err());
    }
    #[test]
    fn nested_plan_metrics_and_collapse() {
        let mut plan = QueryPlan::parse(br#"[{"Plan":{"Node Type":"Nested Loop","Total Cost":10,"Plans":[{"Node Type":"Seq Scan","Relation Name":"users","Total Cost":5,"Actual Rows":3,"Actual Loops":2,"Filter":"id > 0","Shared Hit Blocks":4},{"Node Type":"Result","Total Cost":0}]},"Planning Time":0.2,"Execution Time":1.5}]"#).unwrap();
        assert_eq!(plan.nodes.len(), 3);
        assert_eq!(plan.nodes[1].depth, 1);
        assert_eq!(plan.nodes[1].cost_fraction, 0.5);
        assert!(plan.nodes[1].details.contains("Actual Loops: 2"));
        assert!(plan.nodes[1].details.contains("Filter: id > 0"));
        assert!(plan.summary.contains("Execution Time: 1.500 ms"));
        plan.nodes[0].collapsed = true;
        assert_eq!(plan.visible_indices(), vec![0]);
        plan.nodes[0].collapsed = false;
        assert_eq!(plan.visible_indices(), vec![0, 1, 2]);
    }
    #[test]
    fn estimated_and_invalid_plans() {
        let plan = QueryPlan::parse(br#"[{"Plan":{"Node Type":"Result"}}]"#).unwrap();
        assert!(plan.summary.contains("not executed"));
        assert_eq!(plan.nodes[0].cost_fraction, 0.0);
        for input in [
            "garbage",
            "[]",
            "[{}]",
            r#"[{"Plan":{}}]"#,
            r#"[{"Plan":{"Node Type":"Result","Plans":{}}}]"#,
        ] {
            assert!(QueryPlan::parse(input.as_bytes()).is_err());
        }
    }
}
