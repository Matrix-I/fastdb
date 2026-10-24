use anyhow::Result;
use gpui::*;
use tracing::info;

mod workspace;
mod assets;
use workspace::Workspace;
use assets::FastDbAssetSource;

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
