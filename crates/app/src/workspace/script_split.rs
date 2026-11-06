use gpui::{DispatchPhase, MouseButton, MouseMoveEvent, ScrollHandle};

pub(super) const SPLITTER_HEIGHT: f32 = 5.0;

const DEFAULT_EDITOR_FRACTION: f32 = 0.45;
const MIN_EDITOR_FRACTION: f32 = 0.15;
const MAX_EDITOR_FRACTION: f32 = 0.85;

/// Transient split state belongs to the script tab, independently of its selected result.
pub(super) struct ScriptSplit {
    pub bounds: ScrollHandle,
    pub editor_fraction: f32,
    drag: Option<SplitDrag>,
}

struct SplitDrag {
    mouse_y: f32,
    fraction: f32,
    height: f32,
}

impl Default for ScriptSplit {
    fn default() -> Self {
        Self {
            bounds: ScrollHandle::new(),
            editor_fraction: DEFAULT_EDITOR_FRACTION,
            drag: None,
        }
    }
}

impl ScriptSplit {
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn begin_drag(&mut self, mouse_y: f32, height: f32) {
        self.drag =
            (mouse_y.is_finite() && height.is_finite() && height > 0.0).then_some(SplitDrag {
                mouse_y,
                fraction: self.editor_fraction,
                height,
            });
    }

    pub fn drag_to(&mut self, mouse_y: f32) -> bool {
        let Some(drag) = &self.drag else {
            return false;
        };
        if !mouse_y.is_finite() {
            return false;
        }
        let fraction = (drag.fraction + (mouse_y - drag.mouse_y) / drag.height)
            .clamp(MIN_EDITOR_FRACTION, MAX_EDITOR_FRACTION);
        let changed = fraction != self.editor_fraction;
        self.editor_fraction = fraction;
        changed
    }

    /// Consume active motion before editor character targets can turn it into selection.
    pub fn handle_mouse_move(&mut self, event: &MouseMoveEvent, phase: DispatchPhase) -> bool {
        if phase != DispatchPhase::Capture || !self.is_dragging() {
            return false;
        }
        if event.pressed_button == Some(MouseButton::Left) {
            self.drag_to(f32::from(event.position.y));
        } else {
            self.end_drag();
        }
        true
    }

    pub fn end_drag(&mut self) -> bool {
        self.drag.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptSplit;

    #[test]
    fn active_motion_is_consumed_in_capture_before_editor_selection() {
        use gpui::{DispatchPhase, MouseButton, MouseMoveEvent, point, px};
        let mut split = ScriptSplit::default();
        let mut event = MouseMoveEvent {
            position: point(px(100.0), px(300.0)),
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        };
        assert!(!split.handle_mouse_move(&event, DispatchPhase::Capture));
        split.begin_drag(400.0, 1000.0);
        assert!(!split.handle_mouse_move(&event, DispatchPhase::Bubble));
        assert_eq!(split.editor_fraction, 0.45);
        assert!(split.handle_mouse_move(&event, DispatchPhase::Capture));
        assert!((split.editor_fraction - 0.35).abs() < 0.0001);
        // Even motion at a clamp must be consumed so it never selects editor text.
        event.position.y = px(-10000.0);
        assert!(split.handle_mouse_move(&event, DispatchPhase::Capture));
        assert!(split.handle_mouse_move(&event, DispatchPhase::Capture));
        event.pressed_button = None;
        assert!(split.handle_mouse_move(&event, DispatchPhase::Capture));
        assert!(!split.is_dragging());
        assert!(!split.handle_mouse_move(&event, DispatchPhase::Capture));
    }

    #[test]
    fn dragging_down_grows_editor_and_dragging_up_grows_results() {
        let mut split = ScriptSplit::default();
        split.begin_drag(400.0, 1000.0);
        assert!(split.drag_to(500.0));
        assert!((split.editor_fraction - 0.55).abs() < 0.0001);
        assert!(split.drag_to(300.0));
        assert!((split.editor_fraction - 0.35).abs() < 0.0001);
        assert!(!split.drag_to(300.0));
    }

    #[test]
    fn dragging_is_bounded_and_release_stops_resizing() {
        let mut split = ScriptSplit::default();
        split.begin_drag(400.0, 1000.0);
        split.drag_to(10000.0);
        assert_eq!(split.editor_fraction, 0.85);
        split.drag_to(-10000.0);
        assert_eq!(split.editor_fraction, 0.15);
        assert!(split.end_drag());
        assert!(!split.is_dragging());
        assert!(!split.drag_to(400.0));
        assert_eq!(split.editor_fraction, 0.15);
        assert!(!split.end_drag());
    }

    #[test]
    fn invalid_geometry_cannot_start_or_corrupt_a_drag() {
        let mut split = ScriptSplit::default();
        for height in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            split.begin_drag(400.0, height);
            assert!(!split.is_dragging());
            assert!(!split.drag_to(500.0));
        }
        split.begin_drag(f32::NAN, 1000.0);
        assert!(!split.is_dragging());
        split.begin_drag(400.0, 1000.0);
        assert!(!split.drag_to(f32::NAN));
        assert_eq!(split.editor_fraction, 0.45);
    }

    #[test]
    fn each_tab_keeps_its_own_split_and_new_drags_use_the_latest_fraction() {
        let mut first = ScriptSplit::default();
        let second = ScriptSplit::default();
        first.begin_drag(400.0, 1000.0);
        first.drag_to(500.0);
        first.end_drag();
        first.begin_drag(200.0, 500.0);
        first.drag_to(250.0);
        assert!((first.editor_fraction - 0.65).abs() < 0.0001);
        assert_eq!(second.editor_fraction, 0.45);
        assert!(!second.is_dragging());
    }
}
