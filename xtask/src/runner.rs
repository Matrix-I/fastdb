use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::policy::{median, percentile, validate_metrics};

pub struct ChildGuard(pub Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn rss(pid: u32) -> Result<Option<f64>> {
    let mut child = ChildGuard(
        Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let started = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait()? {
            let mut output = String::new();
            child
                .0
                .stdout
                .take()
                .context("missing ps stdout")?
                .read_to_string(&mut output)?;
            if !status.success() || output.trim().is_empty() {
                return Ok(None);
            }
            return Ok(Some(output.trim().parse::<f64>()? / 1024.0));
        }
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "RSS sampler timed out"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn phase_timeout(phase: &str) -> Duration {
    Duration::from_secs(match phase {
        "ready" => 5,
        "query_start" => 75,
        "scroll_start" => 15,
        _ => 10,
    })
}

struct Capture {
    events: BTreeMap<String, (Instant, Value)>,
    rss: Vec<(Instant, f64)>,
    started: Instant,
}

fn monitor(
    mut child: ChildGuard,
    started: Instant,
    timeout: Duration,
    log: &mut File,
    mut sample: impl FnMut(u32) -> Result<Option<f64>>,
) -> Result<Capture> {
    let stdout = child.0.stdout.take().context("missing benchmark stdout")?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let at = Instant::now();
            if tx
                .send((at, line.map(Some).map_err(|error| error.to_string())))
                .is_err()
            {
                return;
            }
        }
        let _ = tx.send((Instant::now(), Ok(None)));
    });
    let result = (|| {
        let expected = ["ready", "query_start", "query_done", "scroll_start", "done"];
        let mut index = 0;
        let mut phase = "launch".to_owned();
        let mut last_event = started;
        let mut capture = Capture {
            events: BTreeMap::new(),
            rss: Vec::new(),
            started,
        };
        loop {
            let now = Instant::now();
            ensure!(
                now.duration_since(started) <= timeout
                    && now.duration_since(last_event) <= phase_timeout(&phase),
                "benchmark timed out in {phase}; keep the desktop unlocked and benchmark window visible"
            );
            if child.0.try_wait()?.is_none()
                && let Some(value) = sample(child.0.id())?
            {
                capture.rss.push((now, value));
            }
            let (at, line) = match rx.recv_timeout(Duration::from_millis(40)) {
                Ok(message) => message,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(error) => bail!("benchmark output reader disconnected: {error}"),
            };
            let Some(line) = line.map_err(anyhow::Error::msg)? else {
                break;
            };
            writeln!(log, "{line}")?;
            log.flush()?;
            let Some(data) = line.strip_prefix("FASTDB_BENCH ") else {
                continue;
            };
            let data: Value = serde_json::from_str(data)?;
            let event = data["event"].as_str().context("missing benchmark event")?;
            ensure!(
                event != "error",
                "benchmark child error: {}",
                data["message"]
            );
            ensure!(
                expected.get(index) == Some(&event),
                "unexpected benchmark event: {event}"
            );
            if event == "ready" {
                ensure!(
                    data["protocol"].as_u64() == Some(1)
                        && data["debug_assertions"].as_bool() == Some(false),
                    "benchmark requires a release binary with matching protocol"
                );
            }
            phase = event.to_owned();
            capture.events.insert(phase.clone(), (at, data));
            last_event = at;
            index += 1;
        }
        let exit_started = Instant::now();
        let status = loop {
            if let Some(status) = child.0.try_wait()? {
                break status;
            }
            ensure!(
                exit_started.elapsed() < Duration::from_secs(10),
                "benchmark exit timed out"
            );
            thread::sleep(Duration::from_millis(10));
        };
        ensure!(
            status.success() && index == expected.len(),
            "incomplete benchmark: exit={status}, expected={:?}",
            expected.get(index)
        );
        Ok(capture)
    })();
    drop(child); // Close stdout and release the reader even on an error/timeout.
    let cleanup_started = Instant::now();
    while !reader.is_finished() && cleanup_started.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(10));
    }
    ensure!(
        reader.is_finished(),
        "benchmark stdout reader did not exit after child termination"
    );
    reader
        .join()
        .map_err(|_| anyhow::anyhow!("benchmark reader panicked"))?;
    result
}

fn summarize(capture: Capture, binary_bytes: u64) -> Result<Value> {
    let (ready_at, ready) = &capture.events["ready"];
    let query_at = capture.events["query_start"].0;
    let result = &capture.events["query_done"].1;
    ensure!(
        result["rows"].as_u64() == Some(1_000_000) && result["columns"].as_u64() == Some(20),
        "query workload mismatch"
    );
    let frames = capture.events["done"].1["frame_intervals_ms"]
        .as_array()
        .context("missing frame intervals")?
        .iter()
        .map(|v| v.as_f64().context("invalid frame interval"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        frames.len() == 300 && frames.iter().all(|v| v.is_finite() && *v > 0.0),
        "incomplete or invalid scrolling measurements"
    );
    ensure!(
        capture.rss.iter().all(|(_, v)| v.is_finite() && *v >= 0.0),
        "invalid RSS sample"
    );
    let idle = capture
        .rss
        .iter()
        .filter(|(at, _)| *at >= *ready_at + Duration::from_millis(500) && *at < query_at)
        .map(|(_, value)| *value)
        .collect::<Vec<_>>();
    ensure!(idle.len() >= 5, "insufficient RSS samples");
    let scale = ready["scale_factor"]
        .as_f64()
        .context("missing display scale")?;
    let viewport = ready["viewport"]
        .as_array()
        .context("missing display viewport")?;
    ensure!(
        scale.is_finite()
            && scale > 0.0
            && viewport.len() == 2
            && viewport
                .iter()
                .all(|v| v.as_f64().is_some_and(|v| v.is_finite() && v > 0.0)),
        "invalid display geometry"
    );
    let metrics = json!({
        "startup_ms": ready_at.duration_since(capture.started).as_secs_f64() * 1000.0,
        "idle_rss_mib": median(&idle),
        "query_ms": result["query_ms"], "first_batch_ms": result["first_batch_ms"],
        "peak_rss_mib": capture.rss.iter().map(|(_, v)| *v).fold(0.0, f64::max),
        "scroll_p95_ms": percentile(&frames, 0.95),
        "scroll_over_50ms_pct": frames.iter().filter(|v| **v > 50.0).count() as f64 / frames.len() as f64 * 100.0,
        "binary_mib": binary_bytes as f64 / 1_048_576.0,
    });
    validate_metrics(&metrics)?;
    Ok(
        json!({"metrics":metrics, "frame_intervals_ms":frames, "rss_samples":capture.rss.len(),
        "display":{"scale_factor":scale, "viewport":viewport}}),
    )
}

pub fn run_once(binary: &Path, timeout: Duration, log_path: &Path) -> Result<Value> {
    let root = tempfile::Builder::new().prefix("fastdb-bench-").tempdir()?;
    fs::create_dir(root.path().join("config"))?;
    let mut executable = binary.to_owned();
    if cfg!(target_os = "macos") {
        let contents = root.path().join("FastDB Benchmark.app/Contents");
        fs::create_dir_all(contents.join("MacOS"))?;
        executable = contents.join("MacOS/fastdb");
        fs::copy(binary, &executable)?;
        fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>fastdb</string>
<key>CFBundleIdentifier</key><string>com.fastdb.benchmark</string>
<key>CFBundleName</key><string>FastDB Benchmark</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSPrincipalClass</key><string>NSApplication</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>"#,
        )?;
    }
    let mut log = File::create(log_path)?;
    let stderr = log.try_clone()?;
    let started = Instant::now();
    let child = Command::new(executable)
        .arg("--benchmark")
        .current_dir(root.path())
        .env("FASTDB_CONFIG_DIR", root.path().join("config"))
        .env("TMPDIR", root.path())
        .env("RUST_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()?;
    let child = ChildGuard(child);
    let _awake = if cfg!(target_os = "macos") {
        Some(ChildGuard(
            Command::new("/usr/bin/caffeinate")
                .args(["-d", "-i", "-u", "-w", &child.0.id().to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        ))
    } else {
        None
    };
    let capture = monitor(child, started, timeout, &mut log, rss)?;
    summarize(capture, fs::metadata(binary)?.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture() {
        let Ok(mode) = std::env::var("FASTDB_XTASK_FIXTURE") else {
            return;
        };
        let emit = |value: Value| {
            println!("FASTDB_BENCH {value}");
            std::io::stdout().flush().unwrap();
        };
        println!(); // Separate protocol output from the test harness's status prefix.
        match mode.as_str() {
            "timeout" => {
                thread::sleep(Duration::from_secs(30));
                return;
            }
            "incomplete" => return,
            "error" => {
                emit(json!({"event":"error", "message":"fixture failure"}));
                thread::sleep(Duration::from_secs(30));
                return;
            }
            "out_of_order" => {
                emit(json!({"event":"query_start"}));
                return;
            }
            _ => {}
        }
        emit(
            json!({"event":"ready", "protocol":1, "debug_assertions":mode == "debug", "scale_factor":2.0, "viewport":[1280.0, 801.0]}),
        );
        if mode == "debug" {
            thread::sleep(Duration::from_secs(30));
            return;
        }
        thread::sleep(Duration::from_millis(1800));
        emit(json!({"event":"query_start"}));
        emit(
            json!({"event":"query_done", "rows":if mode == "wrong_rows" {10} else {1000000}, "columns":20, "query_ms":100.0, "first_batch_ms":2.0}),
        );
        emit(json!({"event":"scroll_start"}));
        emit(
            json!({"event":"done", "frame_intervals_ms":vec![16.0; if mode == "short_frames" {299} else {300}]}),
        );
    }

    fn run_fixture(mode: &str, timeout: Duration) -> Result<Value> {
        let root = tempfile::tempdir()?;
        let mut log = File::create(root.path().join("fixture.log"))?;
        let started = Instant::now();
        let child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "runner::tests::fixture",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("FASTDB_XTASK_FIXTURE", mode)
            .stdout(Stdio::piped())
            .stderr(log.try_clone()?)
            .spawn()?;
        let capture = monitor(ChildGuard(child), started, timeout, &mut log, |_| {
            Ok(Some(100.0))
        })?;
        summarize(capture, 1024)
    }

    #[test]
    fn complete_native_subprocess_collects_metrics() {
        let run = run_fixture("complete", Duration::from_secs(5)).unwrap();
        assert_eq!(run["metrics"]["idle_rss_mib"], 100.0);
        assert_eq!(run["metrics"]["scroll_p95_ms"], 16.0);
        assert_eq!(run["metrics"]["scroll_over_50ms_pct"], 0.0);
        assert_eq!(run["frame_intervals_ms"].as_array().unwrap().len(), 300);
    }
    #[test]
    fn incomplete_child_cannot_pass() {
        assert!(
            run_fixture("incomplete", Duration::from_secs(5))
                .unwrap_err()
                .to_string()
                .contains("incomplete")
        );
    }
    #[test]
    fn debug_binary_is_rejected_and_reaped() {
        let start = Instant::now();
        assert!(
            run_fixture("debug", Duration::from_secs(5))
                .unwrap_err()
                .to_string()
                .contains("release binary")
        );
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn timeout_kills_and_reaps_child() {
        let start = Instant::now();
        assert!(
            run_fixture("timeout", Duration::from_millis(100))
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn errors_and_unordered_events_fail_closed() {
        for mode in ["error", "out_of_order"] {
            assert!(run_fixture(mode, Duration::from_secs(5)).is_err());
        }
    }
    #[test]
    fn incomplete_workloads_fail_closed() {
        for mode in ["wrong_rows", "short_frames"] {
            assert!(run_fixture(mode, Duration::from_secs(5)).is_err());
        }
    }
}
