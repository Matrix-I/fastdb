//! Opt-in, fixed-workload desktop benchmark. No normal-launch measurement overhead.
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use db_wire::{DatabaseDriver, SqliteDriver, StreamEvent};
use gpui::*;
use mem_engine::QueryResultPager;
use serde_json::json;

use crate::workspace::{TabState, Workspace};

pub const ROWS: u64 = 1_000_000;
const FRAMES: usize = 360;
const WARMUP: usize = 60;

pub fn enabled() -> bool {
    std::env::args().any(|arg| arg == "--benchmark")
}

pub fn validate_profile() -> Result<()> {
    let path = std::env::var_os("FASTDB_CONFIG_DIR")
        .filter(|p| !p.is_empty())
        .ok_or_else(|| anyhow::anyhow!("--benchmark requires a fresh FASTDB_CONFIG_DIR"))?;
    ensure!(
        std::fs::read_dir(path)?.next().is_none(),
        "benchmark config must be empty"
    );
    ensure!(
        !std::path::Path::new(".fastdb_config.json").exists(),
        "benchmark cwd contains legacy config"
    );
    Ok(())
}

fn emit(value: serde_json::Value) {
    println!("FASTDB_BENCH {value}");
    let _ = std::io::stdout().flush();
}

fn query() -> Result<(Arc<QueryResultPager>, f64, f64)> {
    let driver = SqliteDriver::new_memory();
    // Fixed-width, row-varying text plus NULLs; ~700 MB logical data exceeds the
    // default 64 MB resident page cache. No materialized source table or Vec<Row>.
    let columns = (0..20)
        .map(|c| format!("CASE WHEN (n + {c}) % 17 = 0 THEN NULL ELSE printf('%08d-{c:02}-abcdefghijklmnopqrstuvwx', n) END AS c{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "WITH RECURSIVE seq(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM seq WHERE n < {}) SELECT {columns} FROM seq",
        ROWS - 1
    );
    let (tx, rx) = flume::unbounded();
    let start = Instant::now();
    let task =
        db_wire::get_runtime().spawn(async move { driver.execute_streaming(&sql, tx).await });
    let mut pager = None;
    let mut first_batch = None;
    let mut completed = false;
    for event in rx.iter() {
        match event {
            StreamEvent::SchemaReady(p) => pager = Some(p),
            StreamEvent::PageAppended { .. } => {
                first_batch.get_or_insert_with(|| start.elapsed().as_secs_f64() * 1000.0);
            }
            StreamEvent::Completed { total_rows, .. } => {
                ensure!(total_rows == ROWS, "incomplete query");
                completed = true;
            }
            StreamEvent::CommandComplete { .. } | StreamEvent::Notice(_) => {}
            StreamEvent::Error(error) => anyhow::bail!("{error}"),
        }
    }
    db_wire::get_runtime().block_on(task)??;
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    let pager = pager.ok_or_else(|| anyhow::anyhow!("no result pager"))?;
    ensure!(
        completed && pager.total_rows() == ROWS && pager.num_cols() == 20,
        "invalid result shape"
    );
    // Read spilled and recent pages before accepting the measurement.
    ensure!(pager.get_cell(0, 0).is_none(), "NULL decoding changed");
    for row in [1, 500_001, ROWS - 1] {
        let expected = format!("{row:08}-00-abcdefghijklmnopqrstuvwx");
        ensure!(
            pager.get_cell(row, 0).as_deref() == Some(expected.as_bytes()),
            "result data mismatch"
        );
    }
    Ok((
        pager,
        elapsed,
        first_batch.ok_or_else(|| anyhow::anyhow!("no row batch"))?,
    ))
}

pub fn start(workspace: Entity<Workspace>, window: &mut Window, _cx: &mut App) {
    // The first callback runs before the initial draw. Wait one more callback
    // so ready includes that frame; this is not a GPU presentation fence.
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |window, cx| begin(workspace, window, cx));
        window.refresh();
    });
}

fn begin(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
    emit(json!({
        "event": "ready", "protocol": 1,
        "debug_assertions": cfg!(debug_assertions),
        "scale_factor": window.scale_factor(),
        "viewport": [f32::from(window.viewport_size().width), f32::from(window.viewport_size().height)]
    }));
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            emit(json!({"event": "query_start"}));
            let result = cx.background_executor().spawn(async { query() }).await;
            match result {
                Ok((pager, query_ms, first_batch_ms)) => {
                    emit(json!({
                        "event": "query_done", "query_ms": query_ms,
                        "first_batch_ms": first_batch_ms, "rows": ROWS, "columns": 20
                    }));
                    if let Err(error) = cx.update(|window, cx| {
                        cx.activate(true);
                        window.activate_window();
                        workspace.update(cx, |ws, cx| {
                            let cache =
                                sql_intel::SchemaCache::new().expect("benchmark schema cache");
                            let mut tab = TabState::table(
                                1,
                                "main",
                                "benchmark",
                                ws.theme.clone(),
                                Arc::new(cache),
                            );
                            tab.grid.set_pager(pager);
                            ws.tabs = vec![tab];
                            ws.active_tab = 0;
                            cx.notify();
                        });
                        emit(json!({"event": "scroll_start"}));
                        let heartbeat = Arc::new(AtomicUsize::new(0));
                        let watched = heartbeat.clone();
                        // Independent of GPUI's event loop: a stalled renderer must
                        // not leave an unresponsive benchmark window behind.
                        std::thread::spawn(move || {
                            let mut previous = 0;
                            let mut stalled = 0;
                            loop {
                                std::thread::sleep(Duration::from_secs(1));
                                let frame = watched.load(Ordering::Relaxed);
                                if frame == previous { stalled += 1; } else { stalled = 0; }
                                if stalled >= 8 {
                                    emit(json!({"event": "error", "message": "scrolling stalled for 8 seconds; benchmark aborted"}));
                                    std::process::exit(2);
                                }
                                previous = frame;
                            }
                        });
                        scroll(workspace, 0, Instant::now(), Vec::new(), heartbeat, window, cx);
                    }) {
                        emit(json!({"event": "error", "message": error.to_string()}));
                        let _ = cx.update(|_, cx| cx.quit());
                    }
                }
                Err(error) => {
                    emit(json!({"event": "error", "message": format!("{error:#}")}));
                    let _ = cx.update(|_, cx| cx.quit());
                }
            }
        })
        .detach();
}

fn scroll(
    workspace: Entity<Workspace>,
    frame: usize,
    previous: Instant,
    mut intervals: Vec<f64>,
    heartbeat: Arc<AtomicUsize>,
    window: &mut Window,
    cx: &mut App,
) {
    heartbeat.store(frame + 1, Ordering::Relaxed);
    let now = Instant::now();
    if frame > WARMUP {
        intervals.push(now.duration_since(previous).as_secs_f64() * 1000.0);
    }
    if frame == FRAMES {
        if !workspace.read(cx).tabs[0]
            .grid
            .vertical_scroll_handle
            .is_scrollable()
        {
            emit(json!({"event": "error", "message": "benchmark grid was not laid out"}));
            cx.quit();
            return;
        }
        emit(json!({"event": "done", "frame_intervals_ms": intervals}));
        cx.quit();
        return;
    }
    workspace.update(cx, |ws, cx| {
        let grid = &ws.tabs[0].grid;
        // Sequential motion then deterministic distant seeks force spill reads.
        let row = if frame < 210 {
            frame * 8
        } else {
            (frame * 7919) % (ROWS as usize - 100)
        };
        grid.vertical_scroll_handle
            .scroll_to_item_strict(row, ScrollStrategy::Top);
        grid.horizontal_scroll_handle
            .set_offset(point(px(-((frame % 20) as f32 * 75.0)), px(0.0)));
        cx.notify();
    });
    window.on_next_frame(move |window, cx| {
        scroll(workspace, frame + 1, now, intervals, heartbeat, window, cx);
    });
    window.refresh();
}
