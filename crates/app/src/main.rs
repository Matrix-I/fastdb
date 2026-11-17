use anyhow::Result;
use gpui::*;
use tracing::info;

mod assets;
mod benchmark;
mod native_backup;
mod sql_log;
pub mod updater;
mod workspace;
use assets::FastDbAssetSource;
use ui_core::actions::{
    About, CheckForUpdates, Quit, ResetZoom, ToggleSidebar, ToggleTheme, ZoomIn, ZoomOut,
};
use workspace::Workspace;

fn about(_: &About, cx: &mut App) {
    // Menu actions run while the active window is borrowed for dispatch.
    // Defer the update until that borrow has been released.
    cx.defer(|cx| {
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<Workspace>() {
                if let Err(error) = handle.update(cx, |workspace, window, cx| {
                    window.activate_window();
                    window.focus(&workspace.focus_handle);
                    workspace.show_about_dialog = true;
                    cx.notify();
                }) {
                    tracing::warn!("Failed to open About dialog: {error}");
                }
                break;
            }
        }
    });
}

fn quit(_: &Quit, cx: &mut App) {
    info!("Closing FastDB via Quit action");
    // Menu dispatch still borrows the active window. Wait until it is released
    // before entering the workspace's save-before-quit path.
    cx.defer(|cx| {
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<Workspace>() {
                if let Err(error) = handle.update(cx, |workspace, _, cx| workspace.request_quit(cx))
                {
                    tracing::warn!("Failed to request Quit: {error}");
                }
                return;
            }
        }
        cx.quit();
    });
}

fn check_for_updates(_: &CheckForUpdates, cx: &mut App) {
    info!("Triggering Check for Updates from App menu");
    cx.defer(|cx| {
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<Workspace>() {
                if let Err(error) = handle.update(cx, |workspace, window, cx| {
                    window.activate_window();
                    window.focus(&workspace.focus_handle);
                    workspace.trigger_update_check(true, cx);
                }) {
                    tracing::warn!("Failed to open update check: {error}");
                }
                break;
            }
        }
    });
}

fn toggle_theme(_: &ToggleTheme, cx: &mut App) {
    cx.defer(|cx| {
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<Workspace>() {
                let _ = handle.update(cx, |workspace, _, cx| {
                    workspace.toggle_theme(cx);
                });
                break;
            }
        }
    });
}

fn toggle_sidebar(_: &ToggleSidebar, cx: &mut App) {
    cx.defer(|cx| {
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<Workspace>() {
                let _ = handle.update(cx, |workspace, _, cx| {
                    workspace.toggle_sidebar(cx);
                });
                break;
            }
        }
    });
}

fn zoom_in(_: &ZoomIn, cx: &mut App) {
    cx.defer(|cx| {
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<Workspace>() {
                let _ = handle.update(cx, |workspace, _, cx| {
                    workspace.zoom_in_editor(cx);
                });
                break;
            }
        }
    });
}

fn zoom_out(_: &ZoomOut, cx: &mut App) {
    cx.defer(|cx| {
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<Workspace>() {
                let _ = handle.update(cx, |workspace, _, cx| {
                    workspace.zoom_out_editor(cx);
                });
                break;
            }
        }
    });
}

fn reset_zoom(_: &ResetZoom, cx: &mut App) {
    cx.defer(|cx| {
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<Workspace>() {
                let _ = handle.update(cx, |workspace, _, cx| {
                    workspace.reset_editor_zoom(cx);
                });
                break;
            }
        }
    });
}

fn protect_postgres_sql_logs(
    mut filter: tracing_subscriber::EnvFilter,
) -> tracing_subscriber::EnvFilter {
    // These transport modules log raw SQL or bound values, including role passwords.
    for directive in [
        "tokio_postgres::prepare=off",
        "tokio_postgres::query=off",
        "tokio_postgres::simple_query=off",
    ] {
        filter = filter.add_directive(directive.parse().expect("static log filter"));
    }
    filter
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(protect_postgres_sql_logs(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        ))
        .init();

    info!("Initializing FastDB - Next-Gen Pure Rust Database Client");

    if benchmark::enabled() {
        benchmark::validate_profile()?;
    }

    // Avoid touching spill files belonging to normal app sessions during benchmarks.
    if !benchmark::enabled()
        && let Err(e) = platform_hal::paths::cleanup_old_spill_files()
    {
        tracing::warn!("Spill cleanup warning: {}", e);
    }

    let sql_log = sql_log::SqlLog::new()?;
    sql_log.install()?;
    let run_log = sql_log.clone();
    let app = Application::new().with_assets(FastDbAssetSource);

    app.run(move |cx: &mut App| {
        let shutdown_log = run_log.clone();
        cx.on_app_quit(move |_| {
            shutdown_log.shutdown();
            async {}
        })
        .detach();
        cx.activate(true);
        cx.on_action(quit);
        cx.on_action(about);
        cx.on_action(check_for_updates);
        cx.on_action(toggle_theme);
        cx.on_action(toggle_sidebar);
        cx.on_action(zoom_in);
        cx.on_action(zoom_out);
        cx.on_action(reset_zoom);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("cmd-shift-t", ToggleTheme, None),
            KeyBinding::new("ctrl-shift-t", ToggleTheme, None),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-b", ToggleSidebar, None),
            KeyBinding::new("cmd-=", ZoomIn, None),
            KeyBinding::new("ctrl-=", ZoomIn, None),
            KeyBinding::new("cmd-+", ZoomIn, None),
            KeyBinding::new("ctrl-+", ZoomIn, None),
            KeyBinding::new("cmd--", ZoomOut, None),
            KeyBinding::new("ctrl--", ZoomOut, None),
            KeyBinding::new("cmd-0", ResetZoom, None),
            KeyBinding::new("ctrl-0", ResetZoom, None),
        ]);
        cx.set_menus(vec![
            Menu {
                name: "FastDB".into(),
                items: vec![
                    MenuItem::action("About FastDB", About),
                    MenuItem::separator(),
                    MenuItem::action("Check for Updates...", CheckForUpdates),
                    MenuItem::separator(),
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Quit FastDB", Quit),
                ],
            },
            Menu {
                name: "View".into(),
                items: vec![
                    MenuItem::action("Toggle Theme", ToggleTheme),
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::separator(),
                    MenuItem::action("Zoom In (Editor)", ZoomIn),
                    MenuItem::action("Zoom Out (Editor)", ZoomOut),
                    MenuItem::action("Reset Zoom (Editor)", ResetZoom),
                ],
            },
        ]);

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1280.0), px(800.0)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("FastDB — Pure Rust Database Client".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.0), px(12.0))),
            }),
            ..Default::default()
        };

        cx.open_window(window_options, |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(cx, run_log.clone()));
            let weak = workspace.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                if benchmark::enabled() {
                    return true;
                }
                weak.update(cx, |workspace, cx| {
                    if workspace.allow_quit {
                        true
                    } else {
                        workspace.request_quit(cx);
                        false
                    }
                })
                .unwrap_or(true)
            });
            if benchmark::enabled() {
                benchmark::start(workspace.clone(), window, cx);
            }
            workspace
        })
        .expect("Failed to open FastDB main window");
    });

    sql_log.shutdown();
    Ok(())
}

#[cfg(test)]
mod logging_tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::util::SubscriberInitExt;

    #[test]
    fn postgres_sql_transport_debug_is_filtered_even_when_explicitly_requested() {
        let filter = tracing_subscriber::EnvFilter::new(
            "debug,tokio_postgres::prepare=trace,tokio_postgres::simple_query=trace,tokio_postgres::query=trace",
        );
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(super::protect_postgres_sql_logs(filter))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            assert!(!tracing::enabled!(target: "tokio_postgres::prepare", tracing::Level::DEBUG));
            assert!(
                !tracing::enabled!(target: "tokio_postgres::simple_query", tracing::Level::DEBUG)
            );
            assert!(!tracing::enabled!(target: "tokio_postgres::query", tracing::Level::DEBUG));
            assert!(tracing::enabled!(target: "fastdb", tracing::Level::DEBUG));
        });
    }

    #[test]
    fn postgres_administration_query_text_stays_out_of_debug_output() {
        let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
            assert!(std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none());
            eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
            return;
        };
        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl Write for Capture {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().write(buf)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let env = |name| std::env::var(format!("FASTDB_TEST_PG_{name}")).ok();
        let driver = db_wire::create_driver(db_wire::ConnectionConfig {
            driver: db_wire::DbDriverKind::Postgres,
            host: env("HOST").unwrap_or_else(|| "127.0.0.1".into()),
            port: port.parse().unwrap(),
            database: env("DATABASE").unwrap_or_else(|| "postgres".into()),
            user: env("USER").unwrap_or_else(|| "postgres".into()),
            password: env("PASSWORD"),
            use_unix_socket: false,
            ..Default::default()
        })
        .unwrap();
        const SENTINEL: &str = "fastdb_non_secret_transport_test_sentinel";
        for protected in [false, true] {
            let buffer = Arc::new(Mutex::new(Vec::new()));
            let writer = buffer.clone();
            let filter = tracing_subscriber::EnvFilter::new("debug,tokio_postgres=trace");
            let filter = if protected {
                super::protect_postgres_sql_logs(filter)
            } else {
                filter
            };
            let _logs = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(move || Capture(writer.clone()))
                .finish()
                .set_default();
            let sql = format!("SELECT '{SENTINEL}'");
            let (_send, cancel) = flume::bounded(1);
            let result = db_wire::get_runtime()
                .block_on(driver.execute_administration(&sql, cancel))
                .unwrap();
            assert_eq!(result.rows[0][0].as_deref(), Some(SENTINEL));
            let output = buffer.lock().unwrap();
            assert_eq!(
                String::from_utf8_lossy(&output).contains(SENTINEL),
                !protected
            );
        }
    }
}
