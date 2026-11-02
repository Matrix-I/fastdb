//! Cached text geometry and two-axis caret visibility for the script viewport.
use crate::buffer::RopeyBuffer;
use gpui::*;
use std::collections::HashMap;

pub(super) const LINE_HEIGHT: f32 = 20.0;
pub(super) const GUTTER_WIDTH: f32 = 48.0;
pub(super) const CODE_PADDING: f32 = 12.0;
pub(super) const VERTICAL_PADDING: f32 = 4.0;
pub(super) const MONO_FONT: &str = ".AppleSystemUIFontMonospaced, Menlo, monospace";

#[derive(Default)]
pub(super) struct EditorLayout {
    revision: Option<u64>,
    pub widths: Vec<f32>,
    glyphs: HashMap<char, f32>,
}
impl EditorLayout {
    pub fn update(&mut self, buffer: &RopeyBuffer, window: &Window) {
        if self.revision == Some(buffer.revision()) {
            return;
        }
        let font = Font {
            family: MONO_FONT.into(),
            weight: FontWeight::NORMAL,
            ..window.text_style().font()
        };
        self.widths = (0..buffer.line_count())
            .map(|line| {
                buffer
                    .line(line)
                    .unwrap_or_default()
                    .trim_end_matches(['\r', '\n'])
                    .chars()
                    .map(|ch| {
                        *self.glyphs.entry(ch).or_insert_with(|| {
                            let text: SharedString = if ch == ' ' {
                                "\u{00a0}".into()
                            } else {
                                ch.to_string().into()
                            };
                            let run = TextRun {
                                len: text.len(),
                                font: font.clone(),
                                color: rgba(0xffffffff).into(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            f32::from(
                                window
                                    .text_system()
                                    .shape_line(text, px(13.0), &[run], None)
                                    .width,
                            )
                        })
                    })
                    .sum()
            })
            .collect();
        self.revision = Some(buffer.revision());
    }
    pub fn prefix_width(&self, buffer: &RopeyBuffer, line: usize, col: usize) -> f32 {
        buffer
            .line(line)
            .unwrap_or_default()
            .trim_end_matches(['\r', '\n'])
            .chars()
            .take(col)
            .map(|ch| self.glyphs.get(&ch).copied().unwrap_or(7.8))
            .sum()
    }
    pub fn content_width(&self) -> f32 {
        GUTTER_WIDTH + CODE_PADDING * 2.0 + self.widths.iter().copied().fold(0.0, f32::max) + 2.0
    }
}

pub(super) fn reveal_axis(offset: f32, start: f32, end: f32, viewport: f32, content: f32) -> f32 {
    if viewport <= 0.0 {
        return offset;
    }
    let mut scroll = -offset;
    if start < scroll {
        scroll = start;
    } else if end > scroll + viewport {
        scroll = end - viewport;
    }
    -scroll.clamp(0.0, (content - viewport).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::{CODE_PADDING, EditorLayout, GUTTER_WIDTH, reveal_axis};

    #[test]
    fn long_content_overflows_and_reveals_the_caret_on_either_axis() {
        let layout = EditorLayout {
            widths: vec![40.0, 2000.0, 100.0],
            ..Default::default()
        };
        assert_eq!(
            layout.content_width(),
            GUTTER_WIDTH + CODE_PADDING * 2.0 + 2002.0
        );
        assert_eq!(reveal_axis(0.0, 1700.0, 1702.0, 600.0, 2074.0), -1102.0);
        assert_eq!(reveal_axis(-1102.0, 60.0, 62.0, 600.0, 2074.0), -60.0);
        assert_eq!(reveal_axis(0.0, 800.0, 820.0, 300.0, 2000.0), -520.0);
        assert_eq!(reveal_axis(-520.0, 40.0, 60.0, 300.0, 2000.0), -40.0);
    }

    #[test]
    fn caret_visibility_preserves_scroll_and_clamps_after_resize_or_deletion() {
        assert_eq!(reveal_axis(-100.0, 150.0, 152.0, 500.0, 2000.0), -100.0);
        assert_eq!(reveal_axis(-1500.0, 50.0, 52.0, 500.0, 100.0), 0.0);
        assert_eq!(reveal_axis(0.0, 9999.0, 10001.0, 500.0, 2000.0), -1500.0);
        assert_eq!(reveal_axis(-100.0, 0.0, 2.0, 0.0, 2000.0), -100.0);
    }
}
