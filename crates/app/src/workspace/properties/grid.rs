use gpui::*;
use ui_core::theme::Theme;

pub(super) fn metadata_grid(
    id: &'static str,
    headers: &[(&str, f32)],
    rows: Vec<Vec<String>>,
    empty: &str,
    theme: &Theme,
) -> AnyElement {
    let mut grid = div()
        .id(id)
        .flex()
        .flex_col()
        .flex_shrink_0()
        .w_full()
        .min_w(px(headers.iter().map(|(_, width)| width).sum::<f32>()))
        .text_size(px(11.5));
    let mut header = div()
        .flex()
        .min_h(px(24.0))
        .flex_shrink_0()
        .bg(theme.bg_grid_header)
        .text_color(theme.text_secondary);
    for (label, width) in headers {
        header = header.child(
            div()
                .w(px(*width))
                .flex_shrink_0()
                .px_2()
                .py_1()
                .overflow_hidden()
                .child(label.to_string()),
        );
    }
    grid = grid.child(header);
    if rows.is_empty() {
        grid = grid.child(
            div()
                .p_4()
                .text_color(theme.text_muted)
                .child(empty.to_string()),
        );
    }
    for row in rows {
        let mut view = div()
            .flex()
            .flex_shrink_0()
            .min_h(px(26.0))
            .border_b_1()
            .border_color(theme.border)
            .text_color(theme.text_primary)
            .hover(|s| s.bg(theme.bg_surface_hover));
        for (value, (_, width)) in row.into_iter().zip(headers) {
            view = view.child(
                div()
                    .w(px(*width))
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .overflow_hidden()
                    .child(value),
            );
        }
        grid = grid.child(view);
    }
    grid.into_any_element()
}

/// Tracks share the detail viewport's handle, so wheel and pointer scrolling stay in sync.
pub(in crate::workspace) fn detail_scrollbar(
    handle: ScrollHandle,
    vertical: bool,
    theme: &Theme,
) -> AnyElement {
    let track_color = theme.bg_surface;
    let thumb_color = theme.text_muted;
    let mut track = div().flex_shrink_0().bg(track_color);
    track = if vertical {
        track.w(px(12.0)).h_full()
    } else {
        track.h(px(12.0)).w_full()
    };
    track
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let viewport = handle.bounds().size;
                    let max = handle.max_offset();
                    let offset = handle.offset();
                    let (length, visible, maximum, position) = if vertical {
                        (bounds.size.height, viewport.height, max.height, -offset.y)
                    } else {
                        (bounds.size.width, viewport.width, max.width, -offset.x)
                    };
                    let (start, thumb) = scroll_thumb(
                        length.into(),
                        visible.into(),
                        maximum.into(),
                        position.into(),
                    );
                    if maximum <= px(0.0) {
                        return;
                    }
                    let thumb_bounds = if vertical {
                        Bounds::new(
                            point(bounds.left() + px(2.0), bounds.top() + px(start)),
                            size(px(8.0), px(thumb)),
                        )
                    } else {
                        Bounds::new(
                            point(bounds.left() + px(start), bounds.top() + px(2.0)),
                            size(px(thumb), px(8.0)),
                        )
                    };
                    let mut quad = fill(
                        thumb_bounds,
                        Rgba {
                            a: 0.55,
                            ..thumb_color
                        },
                    );
                    quad.corner_radii = px(3.0).into();
                    window.paint_quad(quad);

                    let seek = move |position: Point<Pixels>, window: &mut Window| {
                        let coordinate = if vertical {
                            position.y - bounds.top()
                        } else {
                            position.x - bounds.left()
                        };
                        let travel = f32::from(length) - thumb;
                        let progress = if travel > 0.0 {
                            ((f32::from(coordinate) - thumb / 2.0) / travel).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let mut offset = handle.offset();
                        if vertical {
                            offset.y = -maximum * progress;
                        } else {
                            offset.x = -maximum * progress;
                        }
                        handle.set_offset(offset);
                        window.refresh();
                    };
                    let click_seek = seek.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble
                            && event.button == MouseButton::Left
                            && bounds.contains(&event.position)
                        {
                            click_seek(event.position, window);
                            cx.stop_propagation();
                        }
                    });
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble
                            && event.pressed_button == Some(MouseButton::Left)
                            && bounds.contains(&event.position)
                        {
                            seek(event.position, window);
                            cx.stop_propagation();
                        }
                    });
                },
            )
            .size_full(),
        )
        .into_any_element()
}

fn scroll_thumb(track: f32, viewport: f32, maximum: f32, offset: f32) -> (f32, f32) {
    let length = (track * viewport / (viewport + maximum).max(1.0))
        .max(24.0)
        .min(track);
    let progress = if maximum > 0.0 {
        (offset / maximum).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (progress * (track - length), length)
}

#[cfg(test)]
mod tests {
    use super::scroll_thumb;

    #[test]
    fn thumb_tracks_both_ends_and_clamps_after_resize() {
        assert_eq!(scroll_thumb(100.0, 100.0, 300.0, 0.0), (0.0, 25.0));
        assert_eq!(scroll_thumb(100.0, 100.0, 300.0, 300.0), (75.0, 25.0));
        assert_eq!(scroll_thumb(100.0, 100.0, 300.0, 500.0), (75.0, 25.0));
        assert_eq!(scroll_thumb(10.0, 20.0, 100.0, 50.0), (0.0, 10.0));
        assert_eq!(scroll_thumb(100.0, 100.0, 0.0, 0.0), (0.0, 100.0));
    }
}
