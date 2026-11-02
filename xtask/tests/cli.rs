use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::process::Command;

#[test]
fn report_check_preserves_exit_codes_and_json_compatibility() {
    let root = tempfile::tempdir().unwrap();
    let policy_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/thresholds.json");
    let bytes = fs::read(&policy_path).unwrap();
    let policy: Value = serde_json::from_slice(&bytes).unwrap();
    let metrics = policy["metrics"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, rule)| (key.clone(), json!(rule["max"].as_f64().unwrap() / 2.0)))
        .collect::<serde_json::Map<_, _>>();
    let report = json!({"protocol":1,"environment":{"machine":"fixture"},"workload":{"runs":3},
        "policy_sha256":format!("{:x}",Sha256::digest(&bytes)), "metrics":metrics});
    let baseline = root.path().join("baseline.json");
    let candidate = root.path().join("candidate.json");
    fs::write(&baseline, serde_json::to_vec(&report).unwrap()).unwrap();
    let check = |value: &Value| {
        fs::write(&candidate, serde_json::to_vec(value).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("benchmark-check")
            .arg("--report")
            .arg(&candidate)
            .arg("--baseline")
            .arg(&baseline)
            .arg("--thresholds")
            .arg(&policy_path)
            .output()
            .unwrap()
    };
    assert_eq!(check(&report).status.code(), Some(0));
    let mut regression = report.clone();
    regression["metrics"]["query_ms"] =
        json!(report["metrics"]["query_ms"].as_f64().unwrap() * 1.2);
    let failed = check(&regression);
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("query_ms:"));
    let mut malformed = report.clone();
    malformed["metrics"]
        .as_object_mut()
        .unwrap()
        .remove("startup_ms");
    assert_eq!(check(&malformed).status.code(), Some(2));
    let mut incompatible = report;
    incompatible["environment"]["machine"] = json!("other-host");
    assert_eq!(check(&incompatible).status.code(), Some(2));
}
