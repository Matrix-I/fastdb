//! Bounded versioned native-plan snapshots. Opening a file never executes SQL.
use anyhow::{Context, Result, ensure};
use db_wire::query_plan::{PlanMetrics, PlanNode, QueryPlan};
use db_wire::{ColumnMeta, DataType, DbDriverKind, QueryResultPager};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

const VERSION: u32 = 1;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ROWS: u64 = 4096;
const MAX_COLUMNS: usize = 32;
const MAX_COLUMN_NAME_BYTES: usize = 128;
const MAX_COMPARISON_NODES: usize = 500;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedPlan {
    version: u32,
    provider: String,
    columns: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
}

impl SavedPlan {
    pub(super) fn capture(pager: &QueryResultPager, provider: DbDriverKind) -> Result<Self> {
        let plan = super::parse_plan_for(Some(provider), pager)?;
        ensure!(
            pager.total_rows() <= MAX_ROWS,
            "Saved plans are limited to {MAX_ROWS} rows"
        );
        ensure!(
            pager.columns.len() <= MAX_COLUMNS,
            "Too many saved-plan columns"
        );
        let mut bytes = 0;
        let mut rows = Vec::new();
        for row in 0..pager.total_rows() {
            let mut cells = Vec::new();
            for col in 0..pager.columns.len() {
                let cell = pager
                    .get_cell(row, col)
                    .map(String::from_utf8)
                    .transpose()?;
                bytes += cell.as_ref().map_or(0, String::len);
                ensure!(bytes as u64 <= MAX_BYTES, "Saved plan exceeds 16 MiB");
                cells.push(cell);
            }
            rows.push(cells);
        }
        Ok(Self {
            version: VERSION,
            provider: plan.provider.name().into(),
            columns: pager
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect(),
            rows,
        })
    }

    fn validate(&self) -> Result<DbDriverKind> {
        ensure!(
            self.version == VERSION,
            "Unsupported saved-plan version {}",
            self.version
        );
        let provider =
            DbDriverKind::from_name(&self.provider).context("Unknown saved-plan provider")?;
        ensure!(
            !self.columns.is_empty() && self.columns.len() <= MAX_COLUMNS,
            "Invalid saved-plan columns"
        );
        ensure!(
            self.rows.len() as u64 <= MAX_ROWS,
            "Too many saved-plan rows"
        );
        ensure!(
            self.columns
                .iter()
                .all(|name| !name.is_empty() && name.len() <= MAX_COLUMN_NAME_BYTES),
            "Invalid saved-plan column name"
        );
        ensure!(
            self.rows.iter().all(|row| row.len() == self.columns.len()),
            "Invalid saved-plan row width"
        );
        let bytes: usize = self
            .rows
            .iter()
            .flatten()
            .filter_map(|cell| cell.as_ref())
            .map(String::len)
            .sum();
        ensure!(bytes as u64 <= MAX_BYTES, "Saved plan exceeds 16 MiB");
        if !provider.uses_odbc() && provider != DbDriverKind::Sqlite {
            ensure!(
                self.rows.len() == 1 && self.columns.len() == 1,
                "Expected one native plan cell"
            );
        }
        Ok(provider)
    }

    pub(super) fn restore(&self) -> Result<(Arc<QueryResultPager>, QueryPlan)> {
        let provider = self.validate()?;
        let pager = Arc::new(QueryResultPager::new(
            self.columns
                .iter()
                .map(|name| ColumnMeta::new(name, DataType::Text))
                .collect(),
        )?);
        for row in &self.rows {
            pager.append_row(row.iter().map(|cell| cell.as_deref()))?;
        }
        let plan = super::parse_plan_for(Some(provider), &pager)?;
        ensure!(
            plan.provider == provider,
            "Saved-plan provider does not match native output"
        );
        Ok((pager, plan))
    }
}

pub(super) fn read(path: &Path) -> Result<SavedPlan> {
    ensure!(
        std::fs::metadata(path)?.is_file(),
        "Choose a regular plan JSON file"
    );
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.is_file(),
        "Choose a regular plan JSON file"
    );
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_BYTES, "Saved plan exceeds 16 MiB");
    let saved: SavedPlan = serde_json::from_slice(&bytes).context("Invalid saved-plan JSON")?;
    saved.validate()?;
    Ok(saved)
}

struct LimitedWriter<W> {
    writer: W,
    written: u64,
}
impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > MAX_BYTES - self.written {
            return Err(std::io::Error::other("Saved plan exceeds 16 MiB"));
        }
        let count = self.writer.write(bytes)?;
        self.written += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

pub(super) fn write(path: &Path, saved: &SavedPlan) -> Result<()> {
    saved.restore()?;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    {
        let mut writer = LimitedWriter {
            writer: std::io::BufWriter::new(temporary.as_file_mut()),
            written: 0,
        };
        serde_json::to_writer(&mut writer, saved)?;
        writer.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

type PathInterner<'a> = BTreeMap<(usize, usize, &'a str), usize>;

// Canonical parent IDs share ancestry without copying it for every descendant.
// Child ordinal is significant: swapping nested-loop inputs changes the plan.
fn paths<'a>(
    plan: &'a QueryPlan,
    interner: &mut PathInterner<'a>,
) -> BTreeMap<usize, &'a PlanNode> {
    let mut result = BTreeMap::new();
    let mut ancestors = Vec::new();
    let mut children = BTreeMap::new();
    for node in &plan.nodes {
        ancestors.truncate(node.depth);
        let parent = ancestors.last().copied().unwrap_or(0);
        let ordinal = children.entry(parent).or_insert(0);
        *ordinal += 1;
        let next_id = interner.len() + 1;
        let id = *interner
            .entry((parent, *ordinal, node.title.as_str()))
            .or_insert(next_id);
        result.insert(id, node);
        ancestors.push(id);
    }
    result
}

fn preview(text: &str, limit: usize) -> String {
    let mut characters = text.chars();
    let mut text: String = characters.by_ref().take(limit).collect();
    if characters.next().is_some() {
        text.push_str("… [truncated; full details in native plan]");
    }
    text
}

fn metrics(metrics: &PlanMetrics) -> String {
    let value = |value: Option<f64>| value.map_or_else(|| "unavailable".into(), |n| n.to_string());
    format!(
        "estimated cost={} rows={}; actual rows/loop={} loops={} first ms/loop={} total ms/loop={}",
        value(metrics.estimated_cost),
        value(metrics.estimated_rows),
        value(metrics.actual_rows_per_loop),
        value(metrics.actual_loops),
        value(metrics.actual_first_ms_per_loop),
        value(metrics.actual_total_ms_per_loop)
    )
}

pub(super) fn compare(current: &QueryPlan, saved: &QueryPlan) -> Result<String> {
    ensure!(
        current.provider == saved.provider,
        "Compare plans from the same provider; engine costs are not interchangeable"
    );
    let mut interner = PathInterner::new();
    let current_nodes = paths(current, &mut interner);
    let saved_nodes = paths(saved, &mut interner);
    let mut output = vec![format!(
        "Current: {}\nSaved: {}",
        current.summary, saved.summary
    )];
    let mut changes = 0;
    for (path, node) in &current_nodes {
        let previous = saved_nodes.get(path);
        if previous.is_some_and(|old| old.metrics == node.metrics && old.details == node.details) {
            continue;
        }
        changes += 1;
        if changes <= MAX_COMPARISON_NODES {
            output.push(format!(
                "{}: {}\nCurrent: {}\nSaved: {}\nCurrent details: {}\nSaved details: {}",
                if previous.is_some() {
                    "Changed"
                } else {
                    "Added"
                },
                preview(&node.title, 256),
                metrics(&node.metrics),
                previous.map_or_else(|| "absent".into(), |old| metrics(&old.metrics)),
                preview(&node.details, 1024),
                previous.map_or_else(|| "absent".into(), |old| preview(&old.details, 1024))
            ));
        }
    }
    for (path, node) in &saved_nodes {
        if current_nodes.contains_key(path) {
            continue;
        }
        changes += 1;
        if changes <= MAX_COMPARISON_NODES {
            output.push(format!(
                "Removed: {}\nSaved: {}",
                preview(&node.title, 256),
                metrics(&node.metrics)
            ));
        }
    }
    if changes == 0 {
        output.push("No operator, detail or metric changes.".into());
    }
    if changes > MAX_COMPARISON_NODES {
        output.push(format!(
            "{} changed nodes omitted; report shows the first {MAX_COMPARISON_NODES}",
            changes - MAX_COMPARISON_NODES
        ));
    }
    Ok(output.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::{MAX_BYTES, SavedPlan, compare, read, write};
    use db_wire::query_plan::QueryPlan;
    use db_wire::{ColumnMeta, DataType, QueryResultPager};

    fn raw(json: &str) -> QueryResultPager {
        let pager =
            QueryResultPager::new(vec![ColumnMeta::new("QUERY PLAN", DataType::Text)]).unwrap();
        pager.append_row([Some(json)]).unwrap();
        pager
    }

    #[test]
    fn native_plan_file_roundtrip_preserves_output_and_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.json");
        let json = r#"[{"Plan":{"Node Type":"Seq Scan","Relation Name":"名","Plan Rows":9007199254740993}}]"#;
        let snapshot = SavedPlan::capture(&raw(json), db_wire::DbDriverKind::Postgres).unwrap();
        write(&path, &snapshot).unwrap();
        let (pager, plan) = read(&path).unwrap().restore().unwrap();
        assert_eq!(pager.get_cell(0, 0).unwrap(), json.as_bytes());
        assert!(plan.nodes[0].title.contains("名"));
        let mut mismatched = read(&path).unwrap();
        mismatched.provider = "MySql".into();
        assert!(
            mismatched
                .restore()
                .err()
                .unwrap()
                .to_string()
                .contains("provider")
        );
        assert!(write(&path, &mismatched).is_err());
        assert_eq!(
            read(&path)
                .unwrap()
                .restore()
                .unwrap()
                .0
                .get_cell(0, 0)
                .unwrap(),
            json.as_bytes()
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_versions_shapes_and_oversized_files_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.json");
        for json in [
            r#"{"version":2,"provider":"Postgres","columns":["QUERY PLAN"],"rows":[]}"#,
            r#"{"version":1,"provider":"Postgres","columns":["QUERY PLAN"],"rows":[[]]}"#,
            r#"{"version":1,"provider":"Unknown","columns":[],"rows":[]}"#,
            "garbage",
        ] {
            std::fs::write(&path, json).unwrap();
            assert!(read(&path).is_err());
        }
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(read(&path).unwrap_err().to_string().contains("16 MiB"));
        assert!(read(dir.path()).is_err());
    }

    #[test]
    fn extra_native_plan_rows_are_not_silently_discarded() {
        let pager = raw(r#"[{"Plan":{"Node Type":"Result"}}]"#);
        pager.append_row([Some("unexpected extra output")]).unwrap();
        assert!(SavedPlan::capture(&pager, db_wire::DbDriverKind::Postgres).is_err());
    }

    #[test]
    fn oversized_column_metadata_is_rejected_before_display() {
        let mut saved = SavedPlan::capture(
            &raw(r#"[{"Plan":{"Node Type":"Result"}}]"#),
            db_wire::DbDriverKind::Postgres,
        )
        .unwrap();
        saved.columns[0] = "x".repeat(129);
        assert!(saved.restore().is_err());
    }

    #[test]
    fn escaped_output_limit_preserves_previous_file_and_removes_partial_output() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plan.json");
        std::fs::write(&path, "previous").unwrap();
        let json =
            serde_json::json!([{"Plan":{"Node Type":"\\".repeat(5 * 1024 * 1024)}}]).to_string();
        let saved = SavedPlan::capture(&raw(&json), db_wire::DbDriverKind::Postgres).unwrap();
        let error = write(&path, &saved).unwrap_err();
        assert!(error.to_string().contains("16 MiB"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "previous");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn mariadb_files_retain_provider_and_actual_metrics() {
        let pager = raw(r#"{"query_block":{"select_id":1,"r_loops":2,"r_total_time_ms":6}}"#);
        let saved = SavedPlan::capture(&pager, db_wire::DbDriverKind::MariaDb).unwrap();
        let (_, plan) = saved.restore().unwrap();
        assert_eq!(plan.provider, db_wire::DbDriverKind::MariaDb);
        assert_eq!(plan.nodes[0].metrics.actual_total_ms_per_loop, Some(3.0));
    }

    #[test]
    fn comparison_distinguishes_structure_estimates_and_observations() {
        let old = QueryPlan::parse(br#"[{"Plan":{"Node Type":"Root","Total Cost":5,"Plan Rows":3,"Plans":[{"Node Type":"Removed"}]}}]"#).unwrap();
        let new = QueryPlan::parse(br#"[{"Plan":{"Node Type":"Root","Total Cost":8,"Plan Rows":3,"Actual Rows":2,"Actual Loops":4,"Plans":[{"Node Type":"Added"}]}}]"#).unwrap();
        let report = compare(&new, &old).unwrap();
        assert!(report.contains("Changed: Root"));
        assert!(report.contains("Added: Added"));
        assert!(report.contains("Removed: Removed"));
        assert!(report.contains("estimated cost=8"));
        assert!(report.contains("actual rows/loop=2 loops=4"));
        assert!(compare(&old, &old).unwrap().contains("No operator"));
        let mysql = QueryPlan::parse(br#"{"query_block":{"select_id":1}}"#).unwrap();
        assert!(compare(&old, &mysql).is_err());
    }

    #[test]
    fn comparison_reports_native_sibling_reordering() {
        let old = QueryPlan::parse(
            br#"[{"Plan":{"Node Type":"Root","Plans":[{"Node Type":"A"},{"Node Type":"B"}]}}]"#,
        )
        .unwrap();
        let new = QueryPlan::parse(
            br#"[{"Plan":{"Node Type":"Root","Plans":[{"Node Type":"B"},{"Node Type":"A"}]}}]"#,
        )
        .unwrap();
        let report = compare(&new, &old).unwrap();
        assert!(!report.contains("No operator"));
        assert!(report.contains("Added:"));
    }

    #[test]
    fn deep_native_plan_comparison_preserves_identical_output() {
        use db_wire::{DbDriverKind, query_plan::PlanNode};
        let nodes = (0..4096)
            .map(|depth| PlanNode {
                depth,
                title: "same native operator".into(),
                details: String::new(),
                cost_fraction: 0.0,
                has_children: depth < 4095,
                collapsed: false,
                metrics: Default::default(),
            })
            .collect();
        let plan = QueryPlan::new(DbDriverKind::Sqlite, "deep native hierarchy", nodes);
        assert!(compare(&plan, &plan).unwrap().contains("No operator"));
    }
}
