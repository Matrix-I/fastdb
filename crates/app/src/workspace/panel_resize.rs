use super::*;

/// Default width of the left database navigator
pub(super) const NAVIGATOR_DEFAULT_WIDTH: f32 = 280.0;
pub(super) const NAVIGATOR_MIN_WIDTH: f32 = 180.0;
pub(super) const NAVIGATOR_MAX_WIDTH: f32 = 640.0;
pub(super) const VALUE_PANEL_MIN_WIDTH: f32 = 220.0;
pub(super) const VALUE_PANEL_MAX_WIDTH: f32 = 1000.0;
/// Width the grid area keeps no matter how wide the side panels are dragged
const MIN_CENTER_WIDTH: f32 = 240.0;
/// Fixed chrome beside the grid: center padding and the "PANELS" strip
pub(super) const CENTER_CHROME_WIDTH: f32 = 26.0 + 22.0;
const SPLITTER_WIDTH: f32 = 5.0;

/// A side panel whose inner edge can be dragged to resize it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSplitter {
    Navigator,
    ValuePanel,
}

/// An in-progress splitter drag: which panel, the mouse x and the panel width at mouse down
#[derive(Debug, Clone, Copy)]
pub struct PanelDrag {
    pub splitter: PanelSplitter,
    pub start_mouse_x: f32,
    pub start_width: f32,
}

/// Width of a panel after dragging its splitter by `delta` px, clamped to its own limits
/// and so that the grid keeps `MIN_CENTER_WIDTH` in a window `window_w` wide.
/// `other_w` is the width taken by the opposite side panel (0 when hidden).
pub(super) fn dragged_panel_width(splitter: PanelSplitter, drag: &PanelDrag, delta: f32, window_w: f32, other_w: f32) -> f32 {
    let (min, max, w) = match splitter {
        // The navigator grows to the right, the value panel to the left
        PanelSplitter::Navigator => (NAVIGATOR_MIN_WIDTH, NAVIGATOR_MAX_WIDTH, drag.start_width + delta),
        PanelSplitter::ValuePanel => (VALUE_PANEL_MIN_WIDTH, VALUE_PANEL_MAX_WIDTH, drag.start_width - delta),
    };
    let room = window_w - other_w - CENTER_CHROME_WIDTH - MIN_CENTER_WIDTH;
    w.min(max).min(room).max(min)
}

impl Workspace {
    /// Thin drag handle laid over a panel's inner edge (right edge of the navigator,
    /// left edge of the value panel); the parent must be `relative()`
    pub(super) fn render_panel_splitter(&self, splitter: PanelSplitter, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let is_active = self.resizing_panel.map(|d| d.splitter) == Some(splitter);
        let bg = if is_active {
            Rgba { a: 0.85, ..theme.border_focused }
        } else {
            Rgba { a: 0.0, ..theme.border_focused }
        };
        let hover_bg = Rgba { a: 0.6, ..theme.border_focused };
        let (id, start_width) = match splitter {
            PanelSplitter::Navigator => ("navigator-splitter", self.navigator_width),
            PanelSplitter::ValuePanel => ("value-panel-splitter", self.value_panel_width),
        };

        let handle = div()
            .id(id)
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(SPLITTER_WIDTH))
            .cursor_col_resize()
            .bg(bg)
            .hover(move |s| s.bg(hover_bg))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                this.resizing_panel = Some(PanelDrag {
                    splitter,
                    start_mouse_x: f32::from(event.position.x),
                    start_width,
                });
                cx.stop_propagation();
                cx.notify();
            }));
        match splitter {
            PanelSplitter::Navigator => handle.right_0(),
            PanelSplitter::ValuePanel => handle.left_0(),
        }
    }

    /// Applies a splitter drag to the panel width as the mouse moves
    pub(super) fn drag_panel_splitter(&mut self, mouse_x: f32, window_w: f32) {
        let Some(drag) = self.resizing_panel else { return };
        let delta = mouse_x - drag.start_mouse_x;
        match drag.splitter {
            PanelSplitter::Navigator => {
                let other = if self.show_right_panels { self.value_panel_width } else { 0.0 };
                self.navigator_width = dragged_panel_width(drag.splitter, &drag, delta, window_w, other);
            }
            PanelSplitter::ValuePanel => {
                self.value_panel_width = dragged_panel_width(drag.splitter, &drag, delta, window_w, self.navigator_width);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `super::*` brings in `gpui::test`, which shadows the built-in `#[test]`.
    use super::{dragged_panel_width, PanelDrag, PanelSplitter, CENTER_CHROME_WIDTH, MIN_CENTER_WIDTH,
        NAVIGATOR_MAX_WIDTH, NAVIGATOR_MIN_WIDTH, VALUE_PANEL_MIN_WIDTH};

    fn drag(splitter: PanelSplitter, start_width: f32) -> PanelDrag {
        PanelDrag { splitter, start_mouse_x: 300.0, start_width }
    }

    #[test]
    fn navigator_grows_when_dragged_right() {
        let d = drag(PanelSplitter::Navigator, 280.0);
        assert_eq!(dragged_panel_width(d.splitter, &d, 60.0, 1600.0, 0.0), 340.0);
    }

    #[test]
    fn value_panel_grows_when_dragged_left() {
        let d = drag(PanelSplitter::ValuePanel, 340.0);
        assert_eq!(dragged_panel_width(d.splitter, &d, -100.0, 1600.0, 280.0), 440.0);
    }

    #[test]
    fn widths_are_clamped_to_panel_limits() {
        let d = drag(PanelSplitter::Navigator, 280.0);
        assert_eq!(dragged_panel_width(d.splitter, &d, -500.0, 1600.0, 0.0), NAVIGATOR_MIN_WIDTH);
        assert_eq!(dragged_panel_width(d.splitter, &d, 900.0, 3000.0, 0.0), NAVIGATOR_MAX_WIDTH);
        let d = drag(PanelSplitter::ValuePanel, 340.0);
        assert_eq!(dragged_panel_width(d.splitter, &d, 500.0, 1600.0, 280.0), VALUE_PANEL_MIN_WIDTH);
    }

    #[test]
    fn grid_keeps_its_minimum_width() {
        let d = drag(PanelSplitter::ValuePanel, 340.0);
        let w = dragged_panel_width(d.splitter, &d, -900.0, 1200.0, 280.0);
        assert_eq!(w, 1200.0 - 280.0 - CENTER_CHROME_WIDTH - MIN_CENTER_WIDTH);
    }
}
