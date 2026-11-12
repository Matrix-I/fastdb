//! MySQL FORMAT=JSON estimates; native metric names retain their engine meaning.
use super::{PlanMetrics, PlanNode, QueryPlan};
use anyhow::{Context, Result, ensure};
use serde_json::Value;

const MAX_NODES: usize = 4096;

pub(super) fn parse(value: &Value) -> Result<QueryPlan> {
    parse_for(value, crate::DbDriverKind::MySql)
}

pub(super) fn parse_for(value: &Value, provider: crate::DbDriverKind) -> Result<QueryPlan> {
    let root = value
        .get("query_block")
        .context("Missing MySQL query block")?;
    let root_cost = metric(&root["cost_info"]["query_cost"]);
    let mut nodes = Vec::new();
    visit("query_block", root, 0, root_cost, &mut nodes)?;
    let actual = nodes.iter().any(|node| node.metrics.actual_loops.is_some())
        || (provider == crate::DbDriverKind::MariaDb
            && metric(&value["query_optimization"]["r_total_time_ms"]).is_some());
    let summary = format!(
        "{} · {} · native optimizer metrics",
        provider.name(),
        if actual {
            "Actual plan (statement executed)"
        } else {
            "Estimated plan (statement not executed)"
        }
    );
    Ok(QueryPlan::new(provider, summary, nodes))
}

fn metric(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn is_operator(key: &str) -> bool {
    matches!(
        key,
        "query_block"
            | "table"
            | "nested_loop"
            | "ordering_operation"
            | "grouping_operation"
            | "duplicates_removal"
            | "union_result"
            | "query_specifications"
            | "materialized_from_subquery"
            | "attached_subqueries"
            | "having_subqueries"
            | "optimized_away_subqueries"
            | "subqueries"
            | "windowing"
            | "windows"
            | "read_sorted_file"
            | "filesort"
            | "temporary_table"
            | "block-nl-join"
            | "expression_cache"
    )
}

fn visit(
    key: &str,
    value: &Value,
    depth: usize,
    root_cost: Option<f64>,
    nodes: &mut Vec<PlanNode>,
) -> Result<()> {
    ensure!(
        nodes.len() < MAX_NODES,
        "MySQL plan exceeds {MAX_NODES} nodes"
    );
    let list = matches!(
        key,
        "nested_loop"
            | "query_specifications"
            | "attached_subqueries"
            | "having_subqueries"
            | "optimized_away_subqueries"
            | "subqueries"
            | "windows"
    );
    ensure!(
        if list {
            value.is_array()
        } else {
            value.is_object()
        },
        "Invalid MySQL {key} operator"
    );
    let mut title = match key {
        "query_block" => "Query block".into(),
        "nested_loop" => "Nested loop".into(),
        "window" => "Window".into(),
        _ => key.replace('_', " "),
    };
    if key == "query_block"
        && let Some(id) = value.get("select_id")
    {
        title.push_str(&format!(" {id}"));
    }
    if let Some(name) = value.get("table_name").and_then(Value::as_str) {
        title.push_str(&format!(" · {name}"));
    }
    if let Some(access) = value.get("access_type").and_then(Value::as_str) {
        title.push_str(&format!(" · {access}"));
    }
    let mut details = Vec::new();
    let cost = metric(&value["cost_info"]["prefix_cost"])
        .or_else(|| metric(&value["cost_info"]["query_cost"]));
    let index = nodes.len();
    let loops = metric(&value["r_loops"]);
    let total = metric(&value["r_total_time_ms"]);
    nodes.push(PlanNode {
        depth,
        title,
        details: String::new(),
        cost_fraction: match (cost, root_cost) {
            (Some(cost), Some(total)) if total > 0.0 => (cost / total).clamp(0.0, 1.0) as f32,
            _ => 0.0,
        },
        has_children: false,
        collapsed: false,
        metrics: PlanMetrics {
            estimated_cost: cost,
            estimated_rows: metric(&value["rows_produced_per_join"])
                .or_else(|| metric(&value["rows"])),
            actual_rows_per_loop: metric(&value["r_rows"]),
            actual_loops: loops,
            actual_first_ms_per_loop: None,
            actual_total_ms_per_loop: match (total, loops) {
                (Some(total), Some(loops)) if loops > 0.0 => Some(total / loops),
                _ => None,
            },
        },
    });
    if let Some(items) = value.as_array() {
        for item in items {
            let object = item.as_object().context("Invalid MySQL plan list item")?;
            if key == "windows" {
                visit("window", item, depth + 1, root_cost, nodes)?;
                continue;
            }
            for (child_key, child) in object {
                if is_operator(child_key) {
                    visit(child_key, child, depth + 1, root_cost, nodes)?;
                } else {
                    details.push(format!("{child_key}: {child}"));
                }
            }
        }
    } else if let Some(object) = value.as_object() {
        for (child_key, child) in object {
            if is_operator(child_key) {
                visit(child_key, child, depth + 1, root_cost, nodes)?;
            } else {
                let text = child
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| child.to_string());
                details.push(format!("{child_key}: {text}"));
            }
        }
    }
    nodes[index].details = details.join(" · ");
    nodes[index].has_children = nodes.len() > index + 1;
    Ok(())
}
