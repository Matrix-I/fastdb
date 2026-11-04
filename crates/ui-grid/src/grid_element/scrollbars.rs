use super::*;

/// Thumb geometry uses the laid-out track and list, including header/scrollbar space.
pub(super) fn vertical_thumb(
    track: f32,
    viewport: f32,
    max_scroll: f32,
    offset: f32,
) -> (f32, f32) {
    let ratio = (viewport / (viewport + max_scroll).max(1.0)).clamp(0.08, 0.85);
    let height = (track * ratio).max(30.0).min(track);
    let progress = if max_scroll > 0.0 {
        (offset / max_scroll).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (progress * (track - height), height)
}

impl VirtualDataGrid {
    pub(super) fn render_scrolled_area(&self, scroller: AnyElement) -> AnyElement {
        let theme = &self.theme;
        let row_height = theme.row_height;
        let gutter_width = GUTTER_WIDTH;

        let total_rows = self.row_count();

        let total_cols_width: f32 = self.displayed_width();
        let content_total_width = gutter_width + total_cols_width;
        let has_horizontal_overflow = content_total_width > self.viewport_width + 1.0;

        let total_content_height = total_rows as f32 * row_height;
        let has_vertical_overflow = total_content_height > self.viewport_height + 1.0;

        let grid_container = div()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .bg(theme.bg_grid)
            .text_color(theme.text_primary);

        // The header scrolls horizontally in one element and the rows vertically in another,
        // so each would take its own axis from a diagonal swipe. Intercept the wheel for the
        // whole grid instead and move along the dominant axis only.
        // A scroll redraws only the view the grid is rendered in, not the whole window, so
        // views around it that cache their frame are reused.
        let h_handle = self.horizontal_scroll_handle.clone();
        let v_handle = self.vertical_scroll_handle.clone();
        let fetch_wheel = self.auto_fetch_paused.clone();
        let wheel_router = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let view = window.current_view();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !bounds.contains(&event.position) {
                        return;
                    }
                    let delta = event.delta.pixel_delta(window.line_height());
                    let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
                    if dx.abs() > dy.abs() {
                        let max_x = f32::from(h_handle.max_offset().width);
                        let cur = h_handle.offset();
                        h_handle.set_offset(point(
                            px((f32::from(cur.x) + dx).clamp(-max_x, 0.0)),
                            cur.y,
                        ));
                    } else if dy != 0.0 {
                        fetch_wheel.set(false);
                        let state = v_handle.0.borrow();
                        let max_y = f32::from(state.base_handle.max_offset().height);
                        let cur = state.base_handle.offset();
                        state.base_handle.set_offset(point(
                            cur.x,
                            px((f32::from(cur.y) + dy).clamp(-max_y, 0.0)),
                        ));
                    }
                    cx.stop_propagation();
                    cx.notify(view);
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let mut main_area = div()
            .flex()
            .flex_row()
            .flex_1()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(scroller)
            .child(wheel_router);

        // Vertical scrollbar indicator when content exceeds viewport
        if has_vertical_overflow {
            let scrollbar_w = 14.0f32;
            let btn_h = 16.0f32;

            let theme_border = theme.border;
            let theme_bg_header = theme.bg_grid_header;
            let theme_text_muted = theme.text_muted;
            let thumb_bg = Rgba {
                a: 0.4,
                ..theme_text_muted
            };
            let thumb_hover = Rgba {
                a: 0.8,
                ..theme_text_muted
            };

            let handle_up = self.vertical_scroll_handle.clone();
            let handle_down = self.vertical_scroll_handle.clone();
            let handle_track = self.vertical_scroll_handle.clone();
            let fetch_up = self.auto_fetch_paused.clone();
            let fetch_down = self.auto_fetch_paused.clone();
            let fetch_track = self.auto_fetch_paused.clone();

            let v_scrollbar = div()
                .id("grid-vertical-scrollbar")
                .flex()
                .flex_col()
                .flex_none()
                .items_center()
                .w(px(scrollbar_w))
                .h_full()
                .bg(theme_bg_header)
                .border_l_1()
                .border_color(theme_border)
                .child(
                    div()
                        .id("scroll-btn-up")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w_full()
                        .h(px(btn_h))
                        .flex_none()
                        .cursor_pointer()
                        .text_size(px(8.0))
                        .text_color(theme_text_muted)
                        .hover(|s| {
                            s.bg(Rgba {
                                a: 0.15,
                                ..theme_text_muted
                            })
                        })
                        .child("▲")
                        .on_mouse_down(MouseButton::Left, move |_event, window, _app| {
                            fetch_up.set(false);
                            let cur_offset = handle_up.0.borrow().base_handle.offset();
                            let cur_y = f32::from(cur_offset.y);
                            let new_y = (cur_y + 100.0).min(0.0);
                            handle_up
                                .0
                                .borrow()
                                .base_handle
                                .set_offset(point(cur_offset.x, px(new_y)));
                            window.refresh();
                        }),
                )
                .child(
                    div()
                        .id("scroll-track-v")
                        .flex()
                        .flex_1()
                        .w_full()
                        .relative()
                        .cursor_pointer()
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    // Paint after layout so resizing and scrolling use the same
                                    // current-frame bounds and offset as the virtualized list.
                                    let handle = handle_track.0.borrow().base_handle.clone();
                                    let max_scroll = f32::from(handle.max_offset().height);
                                    let (top, height) = vertical_thumb(
                                        f32::from(bounds.size.height),
                                        f32::from(handle.bounds().size.height),
                                        max_scroll,
                                        -f32::from(handle.offset().y),
                                    );
                                    let thumb_bounds = Bounds::new(
                                        point(bounds.left() + px(2.0), bounds.top() + px(top)),
                                        size(px(scrollbar_w - 4.0), px(height)),
                                    );
                                    let color = if thumb_bounds.contains(&window.mouse_position()) {
                                        thumb_hover
                                    } else {
                                        thumb_bg
                                    };
                                    let mut quad = fill(thumb_bounds, color);
                                    quad.corner_radii = px(2.0).into();
                                    window.paint_quad(quad);
                                    window.on_mouse_event(
                                        move |event: &MouseDownEvent, phase, window, _| {
                                            if phase != DispatchPhase::Bubble
                                                || event.button != MouseButton::Left
                                                || !bounds.contains(&event.position)
                                            {
                                                return;
                                            }
                                            let travel = f32::from(bounds.size.height) - height;
                                            fetch_track.set(false);
                                            let ratio = if travel > 0.0 {
                                                ((f32::from(event.position.y - bounds.top())
                                                    - height / 2.0)
                                                    / travel)
                                                    .clamp(0.0, 1.0)
                                            } else {
                                                0.0
                                            };
                                            let offset = handle.offset();
                                            handle.set_offset(point(
                                                offset.x,
                                                px(-ratio * max_scroll),
                                            ));
                                            window.refresh();
                                        },
                                    );
                                },
                            )
                            .size_full(),
                        ),
                )
                .child(
                    div()
                        .id("scroll-btn-down")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w_full()
                        .h(px(btn_h))
                        .flex_none()
                        .cursor_pointer()
                        .text_size(px(8.0))
                        .text_color(theme_text_muted)
                        .hover(|s| {
                            s.bg(Rgba {
                                a: 0.15,
                                ..theme_text_muted
                            })
                        })
                        .child("▼")
                        .on_mouse_down(MouseButton::Left, move |_event, window, _app| {
                            fetch_down.set(false);
                            let cur_offset = handle_down.0.borrow().base_handle.offset();
                            let cur_y = f32::from(cur_offset.y);
                            let max_scroll_y =
                                f32::from(handle_down.0.borrow().base_handle.max_offset().height);
                            let new_y = (cur_y - 100.0).max(-max_scroll_y);
                            handle_down
                                .0
                                .borrow()
                                .base_handle
                                .set_offset(point(cur_offset.x, px(new_y)));
                            window.refresh();
                        }),
                );

            main_area = main_area.child(v_scrollbar);
        }

        let mut grid_container = grid_container.child(main_area);

        // 3. Horizontal Scrollbar indicator when content exceeds viewport
        if has_horizontal_overflow {
            let scroll_x = (-f32::from(self.horizontal_scroll_handle.offset().x)).max(0.0);
            let max_scroll_x = (content_total_width - self.viewport_width).max(1.0);
            let laid_out_max_x = f32::from(self.horizontal_scroll_handle.max_offset().width);
            let ratio_max_x = if laid_out_max_x > 0.0 {
                laid_out_max_x
            } else {
                max_scroll_x
            };
            let scroll_ratio = (scroll_x / ratio_max_x).clamp(0.0, 1.0);

            let scrollbar_h = 14.0f32;
            let btn_w = 20.0f32;
            let track_w = (self.viewport_width - btn_w * 2.0).max(40.0);
            let thumb_ratio = (self.viewport_width / content_total_width).clamp(0.08, 0.85);
            let thumb_width = (track_w * thumb_ratio).max(36.0);
            let max_travel = (track_w - thumb_width).max(0.0);
            let thumb_left = scroll_ratio * max_travel;

            let theme_border = theme.border;
            let theme_bg_header = theme.bg_grid_header;
            let theme_text_muted = theme.text_muted;
            let thumb_bg = Rgba {
                a: 0.4,
                ..theme_text_muted
            };
            let thumb_hover = Rgba {
                a: 0.8,
                ..theme_text_muted
            };

            let handle_left = self.horizontal_scroll_handle.clone();
            let handle_right = self.horizontal_scroll_handle.clone();
            let handle_track = self.horizontal_scroll_handle.clone();

            let scrollbar_bar = div()
                .id("grid-horizontal-scrollbar")
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .w_full()
                .h(px(scrollbar_h))
                .bg(theme_bg_header)
                .border_t_1()
                .border_color(theme_border)
                .child(
                    div()
                        .id("scroll-btn-left")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(btn_w))
                        .h_full()
                        .cursor_pointer()
                        .text_size(px(9.0))
                        .text_color(theme_text_muted)
                        .hover(|s| {
                            s.bg(Rgba {
                                a: 0.15,
                                ..theme_text_muted
                            })
                        })
                        .child("◀")
                        .on_mouse_down(MouseButton::Left, move |_event, window, _app| {
                            let cur_x = f32::from(handle_left.offset().x);
                            let new_x = (cur_x + 250.0).min(0.0);
                            handle_left.set_offset(point(px(new_x), px(0.0)));
                            window.refresh();
                        }),
                )
                .child(
                    div()
                        .id("scroll-track")
                        .flex()
                        .flex_1()
                        .items_center()
                        .h_full()
                        .relative()
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            move |event: &MouseDownEvent, window, _app| {
                                let click_x = f32::from(event.position.x) - btn_w;
                                let target_ratio = (click_x / track_w).clamp(0.0, 1.0);
                                let new_x = -target_ratio * max_scroll_x;
                                handle_track.set_offset(point(px(new_x), px(0.0)));
                                window.refresh();
                            },
                        )
                        .child(
                            div()
                                .id("scroll-thumb")
                                .absolute()
                                .left(px(thumb_left))
                                .w(px(thumb_width))
                                .h(px(8.0))
                                .rounded_xs()
                                .bg(thumb_bg)
                                .hover(move |s| s.bg(thumb_hover)),
                        ),
                )
                .child(
                    div()
                        .id("scroll-btn-right")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(btn_w))
                        .h_full()
                        .cursor_pointer()
                        .text_size(px(9.0))
                        .text_color(theme_text_muted)
                        .hover(|s| {
                            s.bg(Rgba {
                                a: 0.15,
                                ..theme_text_muted
                            })
                        })
                        .child("▶")
                        .on_mouse_down(MouseButton::Left, move |_event, window, _app| {
                            let cur_x = f32::from(handle_right.offset().x);
                            let new_x = (cur_x - 250.0).max(-max_scroll_x);
                            handle_right.set_offset(point(px(new_x), px(0.0)));
                            window.refresh();
                        }),
                );

            grid_container = grid_container.child(scrollbar_bar);
        }

        grid_container.into_any_element()
    }
}
