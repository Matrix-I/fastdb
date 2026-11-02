use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub const METRICS: [&str; 8] = [
    "binary_mib",
    "first_batch_ms",
    "idle_rss_mib",
    "peak_rss_mib",
    "query_ms",
    "scroll_over_50ms_pct",
    "scroll_p95_ms",
    "startup_ms",
];

#[derive(Deserialize)]
pub struct Policy {
    pub protocol: u64,
    pub metrics: BTreeMap<String, Rule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub max: f64,
    pub relative: f64,
    pub noise: f64,
}

impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.protocol == 1, "incompatible benchmark protocol");
        ensure!(
            self.metrics.len() == METRICS.len()
                && METRICS.iter().all(|key| self.metrics.contains_key(*key)),
            "threshold policy must cover every metric"
        );
        for (name, rule) in &self.metrics {
            ensure!(
                [rule.max, rule.relative, rule.noise]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.0),
                "invalid threshold for {name}"
            );
        }
        Ok(())
    }
}

pub fn validate_metrics(value: &Value) -> Result<BTreeMap<String, f64>> {
    let values = value.as_object().context("missing benchmark metrics")?;
    ensure!(
        values.len() == METRICS.len(),
        "missing or unknown benchmark metrics"
    );
    let mut result = BTreeMap::new();
    for name in METRICS {
        let value = values
            .get(name)
            .and_then(Value::as_f64)
            .context("invalid benchmark metric")?;
        ensure!(
            value.is_finite() && value >= 0.0,
            "invalid benchmark metric: {name}"
        );
        ensure!(
            value > 0.0 || name == "scroll_over_50ms_pct",
            "nonpositive metric: {name}"
        );
        result.insert(name.to_owned(), value);
    }
    ensure!(
        result["scroll_over_50ms_pct"] <= 100.0 && result["first_batch_ms"] <= result["query_ms"],
        "inconsistent benchmark measurements"
    );
    Ok(result)
}

pub fn compare(report: &Value, policy: &Policy, baseline: Option<&Value>) -> Result<Vec<String>> {
    let metrics = validate_metrics(&report["metrics"])?;
    ensure!(
        report["protocol"].as_u64() == Some(1) && policy.protocol == 1,
        "incompatible benchmark protocol"
    );
    policy.validate()?;
    let before = if let Some(baseline) = baseline {
        for key in ["protocol", "environment", "workload", "policy_sha256"] {
            let current = report.get(key).with_context(|| format!("missing {key}"))?;
            ensure!(
                baseline.get(key) == Some(current),
                "baseline mismatch: {key}; rerun both revisions under the same conditions"
            );
        }
        Some(validate_metrics(&baseline["metrics"])?)
    } else {
        None
    };
    let mut failures = Vec::new();
    for (name, rule) in &policy.metrics {
        let value = metrics[name];
        if value > rule.max {
            failures.push(format!("{name}: {value:.3} > budget {:.3}", rule.max));
        }
        if let Some(before) = &before {
            let old = before[name];
            let limit = old + (old * rule.relative).max(rule.noise);
            if value > limit {
                failures.push(format!(
                    "{name}: {value:.3} > baseline allowance {limit:.3} (was {old:.3})"
                ));
            }
        }
    }
    Ok(failures)
}

pub fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

pub fn percentile(values: &[f64], fraction: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[(values.len() as f64 * fraction).ceil() as usize - 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (Value, Policy) {
        let policy: Policy =
            serde_json::from_str(include_str!("../../benchmarks/thresholds.json")).unwrap();
        let metrics: BTreeMap<_, _> = policy
            .metrics
            .iter()
            .map(|(k, v)| (k.clone(), v.max / 2.0))
            .collect();
        (
            json!({"protocol":1, "environment":{"machine":"test"}, "workload":{"rows":1000000}, "policy_sha256":"test", "metrics":metrics}),
            policy,
        )
    }

    #[test]
    fn equal_baseline_passes() {
        let (report, policy) = fixture();
        assert!(compare(&report, &policy, Some(&report)).unwrap().is_empty());
    }
    #[test]
    fn every_absolute_budget_is_enforced() {
        let (report, policy) = fixture();
        for (key, rule) in &policy.metrics {
            let mut candidate = report.clone();
            candidate["metrics"][key] = json!(rule.max + 1.0);
            assert!(
                compare(&candidate, &policy, None)
                    .unwrap()
                    .iter()
                    .any(|v| v.starts_with(key))
            );
        }
    }
    #[test]
    fn relative_regression_below_budget_fails() {
        let (report, policy) = fixture();
        let mut candidate = report.clone();
        candidate["metrics"]["query_ms"] =
            json!(report["metrics"]["query_ms"].as_f64().unwrap() * 1.16);
        assert_eq!(
            compare(&candidate, &policy, Some(&report)).unwrap().len(),
            1
        );
    }
    #[test]
    fn noise_floor_and_boundary() {
        let (mut baseline, policy) = fixture();
        baseline["metrics"]["startup_ms"] = json!(10.0);
        let mut report = baseline.clone();
        report["metrics"]["startup_ms"] = json!(35.0);
        assert!(
            compare(&report, &policy, Some(&baseline))
                .unwrap()
                .is_empty()
        );
        report["metrics"]["startup_ms"] = json!(35.01);
        assert_eq!(compare(&report, &policy, Some(&baseline)).unwrap().len(), 1);
    }
    #[test]
    fn incompatible_baselines_are_rejected() {
        let (report, policy) = fixture();
        for key in ["environment", "workload", "protocol", "policy_sha256"] {
            let mut baseline = report.clone();
            baseline[key] = json!("different");
            assert!(compare(&report, &policy, Some(&baseline)).is_err());
        }
    }
    #[test]
    fn malformed_metrics_and_policies_fail_closed() {
        let (report, mut policy) = fixture();
        for invalid in [Value::Null, json!(-1), json!(0), json!(true), json!("100")] {
            let mut metrics = report["metrics"].clone();
            metrics["query_ms"] = invalid;
            assert!(validate_metrics(&metrics).is_err());
        }
        let mut missing = report["metrics"].clone();
        missing.as_object_mut().unwrap().remove("startup_ms");
        assert!(validate_metrics(&missing).is_err());
        policy.metrics.remove("startup_ms");
        assert!(compare(&report, &policy, None).is_err());
    }
    #[test]
    fn p95_and_even_median_match_python_protocol() {
        assert_eq!(
            percentile(&(1..=300).map(f64::from).collect::<Vec<_>>(), 0.95),
            285.0
        );
        assert_eq!(median(&[4.0, 2.0, 1.0, 3.0]), 2.5);
    }
}
