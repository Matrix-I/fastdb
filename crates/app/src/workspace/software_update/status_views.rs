use super::*;
impl UpdateController {
    pub(super) fn render_checking_state(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py_8()
            .gap_3()
            .child(div().text_size(px(28.0)).child("🔄"))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(14.0))
                    .text_color(theme.text_primary)
                    .child("Checking for updates..."),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child("Connecting to GitHub Releases to check for newer versions."),
            )
            .child(
                div().pt_3().child(
                    div()
                        .id("btn-cancel-checking")
                        .px_4()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme.bg_surface_hover)
                        .border_1()
                        .border_color(theme.border)
                        .text_size(px(12.0))
                        .text_color(theme.text_primary)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.bg_surface_selected))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_update_dialog();
                            cx.notify();
                        }))
                        .child("Cancel"),
                ),
            )
            .into_any_element()
    }

    /// Renders the "You're up to date!" state
    pub(super) fn render_up_to_date_state(
        &self,
        current_version: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cur_ver = current_version.to_string();
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py_6()
            .gap_3()
            .child(
                div()
                    .text_size(px(32.0))
                    .text_color(rgb(0x3fb950))
                    .child("✓"),
            )
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(15.0))
                    .text_color(theme.text_primary)
                    .child("FastDB is up to date!"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child(format!(
                        "FastDB v{} is currently the newest version available.",
                        cur_ver
                    )),
            )
            .child(
                div().pt_4().child(
                    div()
                        .id("btn-uptodate-ok")
                        .px_5()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme.text_accent)
                        .text_color(theme.bg_app)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(12.0))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_update_dialog();
                            cx.notify();
                        }))
                        .child("OK"),
                ),
            )
            .into_any_element()
    }

    /// Renders the "Update Check Failed" error state
    pub(super) fn render_error_state(
        &self,
        message: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let err_msg = message.to_string();
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py_6()
            .gap_3()
            .child(
                div()
                    .text_size(px(28.0))
                    .text_color(rgb(0xf85149))
                    .child("⚠"),
            )
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(15.0))
                    .text_color(theme.text_primary)
                    .child("Could Not Check for Updates"),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .px_4()
                    .child(err_msg),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .pt_4()
                    .child(
                        div()
                            .id("btn-error-close")
                            .px_4()
                            .py_1p5()
                            .rounded_md()
                            .bg(theme.bg_surface_hover)
                            .border_1()
                            .border_color(theme.border)
                            .text_size(px(12.0))
                            .text_color(theme.text_primary)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_update_dialog();
                                cx.notify();
                            }))
                            .child("Close"),
                    )
                    .child(
                        div()
                            .id("btn-error-retry")
                            .px_4()
                            .py_1p5()
                            .rounded_md()
                            .bg(theme.text_accent)
                            .text_color(theme.bg_app)
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.trigger_update_check(true, cx);
                                cx.notify();
                            }))
                            .child("Try Again"),
                    ),
            )
            .into_any_element()
    }

    /// Renders the "Update Available" state with release notes and download actions
    pub(super) fn render_update_available_state(
        &self,
        release: &ReleaseInfo,
        download: &DownloadUiState,
        auto_check: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let release_clone = release.clone();
        let html_url = release.html_url.clone();
        let pub_date = if release.published_at.len() >= 10 {
            &release.published_at[..10]
        } else {
            &release.published_at
        };
        let current_pkg_version = env!("CARGO_PKG_VERSION");

        div()
            .flex()
            .flex_col()
            .gap_3()
            // Title & Versions
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(15.0))
                            .text_color(theme.text_primary)
                            .child("A new version of FastDB is available!".to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_secondary)
                            .child(format!(
                                "FastDB {} is now available (you have v{}). Released on {}.",
                                release.tag_name, current_pkg_version, pub_date
                            )),
                    ),
            )
            // Release Notes container
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_primary)
                            .child("Release Notes:"),
                    )
                    .child(
                        div()
                            .id("release-notes-scroll")
                            .h(px(150.0))
                            .overflow_y_scroll()
                            .p_3()
                            .rounded_md()
                            .bg(theme.bg_editor)
                            .border_1()
                            .border_color(theme.border)
                            .text_size(px(12.0))
                            .text_color(theme.text_secondary)
                            .child(if release.body.trim().is_empty() {
                                "No release notes provided for this version.".to_string()
                            } else {
                                release.body.clone()
                            }),
                    ),
            )
            // Download progress or matching package info
            .child(self.render_download_section(release, download, theme, cx))
            // Footer: Checkbox + Actions
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(theme.border)
                    .pt_3()
                    // Startup check toggle
                    .child(
                        div()
                            .id("chk-auto-update-toggle")
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_auto_check_updates();
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(14.0))
                                    .h(px(14.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_xs()
                                    .border_1()
                                    .border_color(if auto_check {
                                        theme.text_accent
                                    } else {
                                        theme.border
                                    })
                                    .bg(if auto_check {
                                        theme.text_accent
                                    } else {
                                        theme.bg_surface_hover
                                    })
                                    .text_size(px(10.0))
                                    .text_color(theme.bg_app)
                                    .child(if auto_check { "✓" } else { "" }),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("Automatically check for updates"),
                            ),
                    )
                    // Action Buttons
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(if matches!(download, DownloadUiState::Complete { .. }) {
                                div()
                                    .id("btn-close-later")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_update_dialog();
                                        cx.notify();
                                    }))
                                    .child("Later")
                            } else {
                                div()
                                    .id("btn-skip-version")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .hover(|s| s.text_color(theme.text_primary))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip_current_version();
                                        cx.notify();
                                    }))
                                    .child("Skip This Version")
                            })
                            .child(
                                div()
                                    .id("btn-view-on-github")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(theme.bg_surface_hover)
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.0))
                                    .text_color(theme.text_primary)
                                    .hover(|s| s.bg(theme.bg_surface_selected))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.open_url(&html_url);
                                    }))
                                    .child("View on GitHub"),
                            )
                            .child(self.render_primary_download_button(
                                &release_clone,
                                download,
                                theme,
                                cx,
                            )),
                    ),
            )
            .into_any_element()
    }
}
