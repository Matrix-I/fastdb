//! PostgreSQL EXPLAIN generation and presentation model.
use crate::DbDriverKind;
use anyhow::{Context, Result, bail};
use serde_json::Value;

pub fn explain_sql(kind: DbDriverKind, statement: &str, analyze: bool) -> Result<String> {
    if kind != DbDriverKind::Postgres {
        bail!("Visual query plans currently support PostgreSQL only");
    }
    if statement.trim().is_empty() {
        bail!("Place the caret in a statement to explain");
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
}

#[derive(Debug)]
pub struct QueryPlan {
    pub summary: String,
    pub nodes: Vec<PlanNode>,
}

impl QueryPlan {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value: Value = serde_json::from_slice(bytes).context("Invalid EXPLAIN JSON")?;
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
        Ok(Self {
            summary: if summary.is_empty() {
                "Estimated plan (statement not executed)".into()
            } else {
                summary
            },
            nodes,
        })
    }

    fn visit(value: &Value, depth: usize, root_cost: f64, nodes: &mut Vec<PlanNode>) -> Result<()> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(explain_sql(DbDriverKind::Sqlite, "SELECT 1", false).is_err());
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
