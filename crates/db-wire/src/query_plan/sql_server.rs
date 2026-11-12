//! SQL Server SHOWPLAN_ALL uses statement-local node IDs and parent links.
use super::{MAX_PLAN_NODES, PlanMetrics, PlanNode, QueryPlan};
use crate::{DbDriverKind, QueryResultPager};
use anyhow::{Context, Result, ensure};
use std::collections::{HashMap, HashSet};

type NodeId = (u32, u32);

struct NativeNode {
    id: NodeId,
    parent: Option<NodeId>,
    node: PlanNode,
}

pub(super) fn parse(pager: &QueryResultPager) -> Result<QueryPlan> {
    let rows = pager.total_rows();
    ensure!(
        rows > 0 && rows <= MAX_PLAN_NODES as u64,
        "SQL Server plan must contain 1–{MAX_PLAN_NODES} rows"
    );
    let column = |name: &str| {
        pager
            .columns
            .iter()
            .position(|column| column.name.eq_ignore_ascii_case(name))
            .with_context(|| format!("Missing SQL Server plan column {name}"))
    };
    let statement = column("StmtId")?;
    let id = column("NodeId")?;
    let parent = column("Parent")?;
    let physical = column("PhysicalOp")?;
    let logical = column("LogicalOp")?;
    let argument = column("Argument")?;
    let estimate = column("EstimateRows")?;
    let cost = column("TotalSubtreeCost")?;
    let statement_text = column("StmtText")?;
    let mut ids = HashSet::new();
    let mut roots = Vec::new();
    let mut children = HashMap::<NodeId, Vec<NativeNode>>::new();

    for row in 0..rows {
        let cell = |column| -> Result<Option<String>> {
            Ok(pager
                .get_cell(row, column)
                .map(String::from_utf8)
                .transpose()?)
        };
        let required_id = |column| -> Result<u32> {
            Ok(cell(column)?
                .context("Missing SQL Server plan node identity")?
                .trim()
                .parse()?)
        };
        let metric = |column| -> Result<Option<f64>> {
            let Some(value) = cell(column)? else {
                return Ok(None);
            };
            let value: f64 = value.trim().parse()?;
            ensure!(
                value.is_finite() && value >= 0.0,
                "Invalid SQL Server plan estimate"
            );
            Ok(Some(value))
        };
        let statement = required_id(statement)?;
        let node_id = (statement, required_id(id)?);
        ensure!(ids.insert(node_id), "Duplicate SQL Server plan node");
        let parent = cell(parent)?
            .map(|value| value.trim().parse::<u32>())
            .transpose()?
            .filter(|parent| *parent != 0)
            .map(|parent| (statement, parent));
        let title = cell(physical)?
            .or(cell(logical)?)
            .unwrap_or_else(|| format!("Statement {statement}"));
        let mut details = cell(argument)?
            .or(cell(statement_text)?)
            .unwrap_or_default();
        let estimated_rows = metric(estimate)?;
        let estimated_cost = metric(cost)?;
        for (label, value) in [
            ("Estimated rows", estimated_rows),
            ("Estimated subtree cost", estimated_cost),
        ] {
            if let Some(value) = value {
                if !details.is_empty() {
                    details.push_str(" · ");
                }
                details.push_str(&format!("{label}: {value}"));
            }
        }
        let node = NativeNode {
            id: node_id,
            parent,
            node: PlanNode {
                depth: 0,
                title,
                details,
                cost_fraction: 0.0,
                has_children: false,
                collapsed: false,
                metrics: PlanMetrics {
                    estimated_rows,
                    estimated_cost,
                    ..Default::default()
                },
            },
        };
        if let Some(parent) = node.parent {
            children.entry(parent).or_default().push(node);
        } else {
            roots.push(node);
        }
    }

    let mut nodes = Vec::with_capacity(rows as usize);
    let mut pending: Vec<_> = roots
        .into_iter()
        .rev()
        .map(|node| {
            let cost = node.node.metrics.estimated_cost;
            (node, 0, cost)
        })
        .collect();
    while let Some((native, depth, root_cost)) = pending.pop() {
        let mut node = native.node;
        node.depth = depth;
        if let (Some(cost), Some(total)) = (node.metrics.estimated_cost, root_cost)
            && total > 0.0
        {
            node.cost_fraction = (cost / total).clamp(0.0, 1.0) as f32;
        }
        if let Some(children) = children.remove(&native.id) {
            node.has_children = true;
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|child| (child, depth + 1, root_cost)),
            );
        }
        nodes.push(node);
    }
    ensure!(
        nodes.len() as u64 == rows,
        "Invalid SQL Server plan hierarchy"
    );
    Ok(QueryPlan::new(
        DbDriverKind::SqlServer,
        "SQL Server · Estimated plan (statement not executed)",
        nodes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnMeta, DataType};

    fn pager() -> QueryResultPager {
        QueryResultPager::new(
            [
                "StmtText",
                "StmtId",
                "NodeId",
                "Parent",
                "PhysicalOp",
                "LogicalOp",
                "Argument",
                "EstimateRows",
                "TotalSubtreeCost",
            ]
            .into_iter()
            .map(|name| ColumnMeta::new(name, DataType::Text))
            .collect(),
        )
        .unwrap()
    }

    #[test]
    fn showplan_keeps_native_order_and_statement_local_ids() {
        let pager = pager();
        for row in [
            [
                Some("SELECT"),
                Some("1"),
                Some("1"),
                Some("0"),
                None,
                None,
                None,
                Some("4"),
                Some("8"),
            ],
            [
                None,
                Some("1"),
                Some("2"),
                Some("1"),
                Some("Index Seek"),
                None,
                Some("items"),
                Some("4"),
                Some("2"),
            ],
            [
                Some("SELECT"),
                Some("2"),
                Some("1"),
                Some("0"),
                None,
                None,
                None,
                None,
                None,
            ],
        ] {
            pager.append_row(row).unwrap();
        }
        let plan = parse(&pager).unwrap();
        assert_eq!(
            plan.nodes.iter().map(|node| node.depth).collect::<Vec<_>>(),
            [0, 1, 0]
        );
        assert_eq!(plan.nodes[1].title, "Index Seek");
        assert!(
            plan.nodes[1]
                .details
                .starts_with("items · Estimated rows: 4")
        );
        assert_eq!(plan.nodes[1].cost_fraction, 0.25);
        assert_eq!(plan.nodes[1].metrics.estimated_rows, Some(4.0));
        assert_eq!(plan.nodes[1].metrics.actual_loops, None);
        assert_eq!(plan.nodes[2].metrics.estimated_cost, None);
    }

    #[test]
    fn showplan_rejects_orphans_and_cycles() {
        let pager = pager();
        for row in [
            [
                None,
                Some("1"),
                Some("1"),
                Some("0"),
                None,
                None,
                None,
                None,
                None,
            ],
            [
                None,
                Some("1"),
                Some("2"),
                Some("3"),
                None,
                None,
                None,
                None,
                None,
            ],
            [
                None,
                Some("1"),
                Some("3"),
                Some("2"),
                None,
                None,
                None,
                None,
                None,
            ],
        ] {
            pager.append_row(row).unwrap();
        }
        assert!(parse(&pager).unwrap_err().to_string().contains("hierarchy"));
    }
}
