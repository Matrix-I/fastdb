mod policy;
mod runner;

use anyhow::{Context, Result, bail, ensure};
use policy::{METRICS, Policy, compare, median};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

struct Options {
    command: String,
    binary: PathBuf,
    output: Option<PathBuf>,
    report: Option<PathBuf>,
    baseline: Option<PathBuf>,
    thresholds: PathBuf,
    runs: usize,
    timeout: Duration,
    machine_id: Option<String>,
    display_id: Option<String>,
}

fn usage() -> &'static str {
    "cargo xtask benchmark --output PATH --display-id ID [--binary PATH] [--baseline PATH]\n  [--thresholds PATH] [--runs 5] [--timeout 180] [--machine-id ID]\ncargo xtask benchmark-check --report PATH [--baseline PATH] [--thresholds PATH]"
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>> {
        let mut args = args.into_iter();
        let command = args.next().context(usage())?;
        if command == "--help" || command == "-h" {
            println!("{}", usage());
            return Ok(None);
        }
        ensure!(
            matches!(command.as_str(), "benchmark" | "benchmark-check"),
            "{}",
            usage()
        );
        let mut options = Self {
            command,
            binary: "target/release/fastdb".into(),
            output: None,
            report: None,
            baseline: None,
            thresholds: Path::new(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/thresholds.json"),
            runs: 5,
            timeout: Duration::from_secs(180),
            machine_id: None,
            display_id: None,
        };
        while let Some(key) = args.next() {
            if key == "--help" || key == "-h" {
                println!("{}", usage());
                return Ok(None);
            }
            let value = args
                .next()
                .with_context(|| format!("missing value for {key}"))?;
            match key.as_str() {
                "--binary" => options.binary = value.into(),
                "--output" => options.output = Some(value.into()),
                "--report" => options.report = Some(value.into()),
                "--baseline" => options.baseline = Some(value.into()),
                "--thresholds" => options.thresholds = value.into(),
                "--runs" => options.runs = value.parse().context("invalid --runs")?,
                "--timeout" => {
                    let seconds: f64 = value.parse().context("invalid --timeout")?;
                    ensure!(
                        seconds.is_finite() && seconds > 0.0,
                        "timeout must be finite and positive"
                    );
                    options.timeout =
                        Duration::try_from_secs_f64(seconds).context("invalid --timeout")?;
                }
                "--machine-id" => options.machine_id = Some(value),
                "--display-id" => options.display_id = Some(value),
                _ => bail!("unknown argument: {key}"),
            }
        }
        ensure!(options.runs >= 3, "use at least 3 measured runs");
        if options.command == "benchmark" {
            ensure!(
                options.output.is_some()
                    && options.display_id.as_ref().is_some_and(|v| !v.is_empty()),
                "benchmark requires --output and --display-id"
            );
            ensure!(
                options.report.is_none(),
                "--report is only for benchmark-check"
            );
        } else {
            ensure!(
                options.report.is_some(),
                "benchmark-check requires --report"
            );
        }
        Ok(Some(options))
    }
}

fn command(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("run {program}"))?;
    ensure!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("read {}", path.display()))?)
        .context("invalid JSON")
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut output = serde_json::to_vec_pretty(value)?;
    output.push(b'\n');
    fs::write(path, output).with_context(|| format!("write {}", path.display()))
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn run(options: Options) -> Result<u8> {
    let bytes = fs::read(&options.thresholds)?;
    let policy: Policy = serde_json::from_slice(&bytes)?;
    policy.validate()?;
    if options.command == "benchmark-check" {
        let report = read_json(options.report.as_deref().context("missing report")?)?;
        let baseline = options.baseline.as_deref().map(read_json).transpose()?;
        ensure!(
            report["policy_sha256"].as_str() == Some(hash(&bytes).as_str()),
            "report threshold policy hash differs"
        );
        return print_failures(compare(&report, &policy, baseline.as_ref())?);
    }
    ensure!(
        cfg!(any(target_os = "macos", target_os = "linux")),
        "desktop benchmarking supports macOS and Linux"
    );
    let output = options.output.context("missing output")?;
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let binary = fs::canonicalize(options.binary)?;
    let mut environment = json!({
        "machine_id": options.machine_id.map(Ok).unwrap_or_else(|| command("uname", &["-n"]))?,
        "display_id": options.display_id,
        "os": command("uname", &["-s"])?, "os_release": command("uname", &["-r"])?,
        "arch": command("uname", &["-m"])?,
        "cpu": if cfg!(target_os = "macos") { command("sysctl", &["-n", "machdep.cpu.brand_string"])? } else { String::new() },
        "rustc": command("rustc", &["--version"])?, "profile":"release",
        "power_assertion": if cfg!(target_os = "macos") { "caffeinate-display-idle" } else { "external" },
    });
    let mut runs = Vec::new();
    for index in 0..=options.runs {
        if index == 0 {
            println!("warmup");
        } else {
            println!("run {index}/{}", options.runs);
        }
        std::io::stdout().flush()?;
        let sample = runner::run_once(
            &binary,
            options.timeout,
            &output.with_extension(format!("run-{index}.log")),
        )?;
        if index > 0 {
            runs.push(sample);
        }
    }
    ensure!(
        runs.iter().all(|r| r["display"] == runs[0]["display"]),
        "display geometry changed between repetitions"
    );
    environment["display"] = runs[0]["display"].clone();
    let metrics: BTreeMap<_, _> = METRICS
        .iter()
        .map(|key| {
            let values = runs
                .iter()
                .map(|r| r["metrics"][key].as_f64().expect("validated metric"))
                .collect::<Vec<_>>();
            (*key, median(&values))
        })
        .collect();
    let mut report = json!({
        "measured_at_utc": chrono::Utc::now().to_rfc3339(), "protocol":1, "environment":environment,
        "workload":{"rows":1000000, "columns":20, "scroll_frames":300, "window":"1280x800", "window_kind":"normal", "runs":options.runs, "discarded_runs":1},
        "policy_sha256":hash(&bytes), "binary_sha256":hash(&fs::read(binary)?),
        "revision":command("git", &["rev-parse", "HEAD"])?,
        "dirty":!command("git", &["-c", "core.fsmonitor=false", "status", "--porcelain", "--untracked-files=normal"])?.is_empty(),
        "metrics":metrics, "runs":runs,
    });
    let comparison = (|| {
        let baseline = options.baseline.as_deref().map(read_json).transpose()?;
        compare(&report, &policy, baseline.as_ref())
    })();
    let failures = match comparison {
        Ok(failures) => failures,
        Err(error) => {
            report["comparison_error"] = json!(error.to_string());
            write_json(&output, &report)?;
            return Err(error);
        }
    };
    report["failures"] = json!(failures);
    write_json(&output, &report)?;
    for (key, value) in &metrics {
        println!("{key}: {value:.3}");
    }
    println!("Report: {}", output.display());
    print_failures(failures)
}
fn print_failures(failures: Vec<String>) -> Result<u8> {
    for failure in &failures {
        eprintln!("FAIL {failure}");
    }
    Ok(u8::from(!failures.is_empty()))
}
fn main() -> ExitCode {
    match Options::parse(std::env::args().skip(1)).and_then(|options| options.map(run).transpose())
    {
        Ok(code) => ExitCode::from(code.unwrap_or(0)),
        Err(error) => {
            eprintln!("Benchmark failed: {error:#}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_rejects_invalid_run_counts_and_deadlines() {
        for args in [
            vec![
                "benchmark",
                "--output",
                "out.json",
                "--display-id",
                "test",
                "--runs",
                "2",
            ],
            vec![
                "benchmark",
                "--output",
                "out.json",
                "--display-id",
                "test",
                "--timeout",
                "NaN",
            ],
            vec![
                "benchmark",
                "--output",
                "out.json",
                "--display-id",
                "test",
                "--timeout",
                "0",
            ],
            vec!["benchmark-check"],
        ] {
            assert!(Options::parse(args.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn sha256_matches_standard_vector() {
        assert_eq!(
            hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
