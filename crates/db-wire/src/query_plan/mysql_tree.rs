//! MySQL 8 TREE runtime output. Native times and row counts are per loop.
use super::{PlanMetrics, PlanNode, QueryPlan};
use crate::DbDriverKind;
use anyhow::{Context, Result, ensure};

const INDENT: usize = 4;
const MAX_NODES: usize = 4096;

fn number(text: &str) -> Result<f64> {
    let number: f64 = text.parse().context("Invalid MySQL TREE metric")?;
    ensure!(
        number.is_finite() && number >= 0.0,
        "Invalid MySQL TREE metric"
    );
    Ok(number)
}

fn field<'a>(text: &'a str, key: &str) -> Result<&'a str> {
    text.split_whitespace()
        .find_map(|part| part.strip_prefix(key))
        .context("Missing MySQL TREE metric")
}

fn metrics<'a>(text: &'a str, metrics: &mut PlanMetrics) -> Result<(&'a str, bool)> {
    let (estimated, observed) = if let Some(start) = text.rfind(" (actual time=") {
        let actual = text[start..]
            .trim()
            .strip_prefix("(actual time=")
            .and_then(|v| v.strip_suffix(')'))
            .context("Invalid MySQL TREE actual metrics")?;
        let (first, total) = actual
            .split_whitespace()
            .next()
            .context("Missing MySQL TREE timing")?
            .split_once("..")
            .context("Invalid MySQL TREE timing range")?;
        metrics.actual_first_ms_per_loop = Some(number(first)?);
        metrics.actual_total_ms_per_loop = Some(number(total)?);
        metrics.actual_rows_per_loop = Some(number(field(actual, "rows=")?)?);
        metrics.actual_loops = Some(number(field(actual, "loops=")?)?);
        (&text[..start], true)
    } else if let Some(estimated) = text.strip_suffix(" (never executed)") {
        (estimated, true)
    } else {
        (text, false)
    };
    if let Some(start) = estimated.rfind(" (cost=") {
        let estimate = estimated[start..]
            .trim()
            .strip_prefix("(cost=")
            .and_then(|v| v.strip_suffix(')'))
            .context("Invalid MySQL TREE cost metrics")?;
        let cost = estimate
            .split_whitespace()
            .next()
            .context("Missing MySQL TREE cost")?;
        let cost = if let Some((first, total)) = cost.split_once("..") {
            number(first)?;
            total
        } else {
            cost
        };
        metrics.estimated_cost = Some(number(cost)?);
        metrics.estimated_rows = Some(number(field(estimate, "rows=")?)?);
        return Ok((estimated[..start].trim_end(), observed));
    }
    Ok((estimated.trim_end(), observed))
}

pub(super) fn parse(text: &str) -> Result<QueryPlan> {
    let mut nodes: Vec<PlanNode> = Vec::new();
    let mut observed = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        ensure!(
            nodes.len() < MAX_NODES,
            "MySQL TREE exceeds {MAX_NODES} nodes"
        );
        let line = line.trim_end();
        let indent = line.len() - line.trim_start_matches(' ').len();
        ensure!(indent % INDENT == 0, "Invalid MySQL TREE indentation");
        let depth = indent / INDENT;
        ensure!(
            if let Some(last) = nodes.last() {
                depth > 0 && depth <= last.depth + 1
            } else {
                depth == 0
            },
            "Invalid MySQL TREE hierarchy"
        );
        let text = line[indent..]
            .strip_prefix("-> ")
            .context("Invalid MySQL TREE operator")?;
        let mut native = PlanMetrics::default();
        let (title, actual) = metrics(text, &mut native)?;
        ensure!(!title.is_empty(), "Missing MySQL TREE operator name");
        observed.push(actual);
        nodes.push(PlanNode {
            depth,
            title: title.into(),
            details: text[title.len()..].trim().into(),
            cost_fraction: 0.0,
            has_children: false,
            collapsed: false,
            metrics: native,
        });
    }
    ensure!(!nodes.is_empty(), "MySQL returned no TREE operators");
    let root_cost = nodes[0].metrics.estimated_cost;
    for i in 0..nodes.len() {
        nodes[i].has_children = nodes
            .get(i + 1)
            .is_some_and(|child| child.depth > nodes[i].depth);
        ensure!(
            observed[i] || nodes[i].has_children,
            "Missing MySQL TREE actual metrics"
        );
        if let (Some(cost), Some(total)) = (nodes[i].metrics.estimated_cost, root_cost)
            && total > 0.0
        {
            nodes[i].cost_fraction = (cost / total).clamp(0.0, 1.0) as f32;
        }
    }
    Ok(QueryPlan::new(
        DbDriverKind::MySql,
        "MySQL · Actual plan · times and rows are per loop; statement executed",
        nodes,
    ))
}
