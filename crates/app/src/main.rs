#![recursion_limit = "1024"]

use anyhow::Result;
use gpui::*;
use tracing::info;

mod workspace;
mod assets;
pub mod updater;
use workspace::Workspace;
use assets::FastDbAssetSource;
use ui_core::actions::{About, CheckForUpdates, Quit};

fn about(_: &About, cx: &mut App) {
    for handle in cx.windows() {
        if let Some(handle) = handle.downcast::<Workspace>() {
            let _ = handle.update(cx, |workspace, window, cx| {
                window.activate_window();
                window.focus(&workspace.focus_handle);
                workspace.show_about_dialog = true;
                cx.notify();
            });
            break;
        }
    }
}

fn quit(_: &Quit, cx: &mut App) {
    info!("Closing FastDB via Quit action");
    cx.quit();
}

fn check_for_updates(_: &CheckForUpdates, cx: &mut App) {
    info!("Triggering Check for Updates from App menu");
    for handle in cx.windows() {
        if let Some(handle) = handle.downcast::<Workspace>() {
            let _ = handle.update(cx, |workspace, window, cx| {
                window.activate_window();
                window.focus(&workspace.focus_handle);
                workspace.trigger_update_check(true, cx);
            });
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Initializing FastDB - Next-Gen Pure Rust Database Client");

    // Clean up abandoned spill files on startup
    if let Err(e) = platform_hal::paths::cleanup_old_spill_files() {
        tracing::warn!("Spill cleanup warning: {}", e);
    }

    let app = Application::new().with_assets(FastDbAssetSource);

    app.run(|cx: &mut App| {
        cx.activate(true);
        cx.on_action(quit);
        cx.on_action(about);
        cx.on_action(check_for_updates);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.set_menus(vec![Menu {
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
        }]);

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

        cx.open_window(window_options, |_window, cx| {
            cx.new(Workspace::new)
        })
        .expect("Failed to open FastDB main window");
    });

    Ok(())
}
