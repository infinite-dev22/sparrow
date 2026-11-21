//! Sticky editor header calculation and rendering.

use floem::{
    Renderer,
    context::PaintCx,
    peniko::kurbo::{Point, Rect, Size},
    reactive::{RwSignal, SignalGet, SignalUpdate},
    views::editor::view::ScreenLines,
};

use crate::config::{SparrowConfig, color::SparrowColor};
use crate::editor::EditorData;

use super::EditorView;

#[derive(Clone, Debug, Default)]
pub struct StickyHeaderInfo {
    pub sticky_lines: Vec<usize>,
    pub last_sticky_should_scroll: bool,
    pub y_diff: f64,
}

pub(super) fn get_sticky_header_info(
    editor_data: &EditorData,
    viewport: RwSignal<Rect>,
    sticky_header_height_signal: RwSignal<f64>,
    config: &SparrowConfig,
) -> StickyHeaderInfo {
    let editor = &editor_data.editor;
    let doc = editor_data.doc();

    let viewport = viewport.get();
    // TODO(minor): should this be a `get`
    let screen_lines = editor.screen_lines.get();
    let line_height = config.editor.line_height() as f64;
    // let start_line = (viewport.y0 / line_height).floor() as usize;
    let Some(start) = screen_lines.lines.first() else {
        return StickyHeaderInfo {
            sticky_lines: Vec::new(),
            last_sticky_should_scroll: false,
            y_diff: 0.0,
        };
    };
    let start_info = screen_lines.info(*start).unwrap();
    let start_line = start_info.vline_info.rvline.line;

    let y_diff = viewport.y0 - start_info.vline_y;

    let mut last_sticky_should_scroll = false;
    let mut sticky_lines = Vec::new();
    if let Some(lines) = doc.sticky_headers(start_line) {
        let total_lines = lines.len();
        if total_lines > 0 {
            let line = start_line + total_lines;
            if let Some(new_lines) = doc.sticky_headers(line) {
                if new_lines.len() > total_lines {
                    sticky_lines = new_lines;
                } else {
                    sticky_lines = lines;
                    last_sticky_should_scroll = new_lines.len() < total_lines;
                    if new_lines.len() < total_lines {
                        if let Some(new_new_lines) =
                            doc.sticky_headers(start_line + total_lines - 1)
                        {
                            if new_new_lines.len() < total_lines {
                                sticky_lines.pop();
                                last_sticky_should_scroll = false;
                            }
                        } else {
                            sticky_lines.pop();
                            last_sticky_should_scroll = false;
                        }
                    }
                }
            } else {
                sticky_lines = lines;
                last_sticky_should_scroll = true;
            }
        }
    }

    let total_sticky_lines = sticky_lines.len();

    let paint_last_line = total_sticky_lines > 0
        && (last_sticky_should_scroll
            || y_diff != 0.0
            || start_line + total_sticky_lines - 1 != *sticky_lines.last().unwrap());

    // Fix up the line count in case we don't need to paint the last one.
    let total_sticky_lines = if paint_last_line {
        total_sticky_lines
    } else {
        total_sticky_lines.saturating_sub(1)
    };

    if total_sticky_lines == 0 {
        sticky_header_height_signal.set(0.0);
        return StickyHeaderInfo {
            sticky_lines: Vec::new(),
            last_sticky_should_scroll: false,
            y_diff: 0.0,
        };
    }

    let scroll_offset = if last_sticky_should_scroll {
        y_diff
    } else {
        0.0
    };

    let sticky_header_height = sticky_lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            // TODO(question): won't y_diff always be scroll_offset here? so we should just sub on
            // the outside
            let y_diff = if i == total_sticky_lines - 1 {
                scroll_offset
            } else {
                0.0
            };

            let layout = editor.text_layout(*line);
            layout.line_count() as f64 * line_height - y_diff
        })
        .sum();

    sticky_header_height_signal.set(sticky_header_height);
    StickyHeaderInfo {
        sticky_lines,
        last_sticky_should_scroll,
        y_diff,
    }
}

impl EditorView {
    pub(super) fn paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
    ) {
        let config = self.editor.common.config.get_untracked();
        if !config.editor.sticky_header {
            return;
        }
        if !self.editor.kind.get_untracked().is_normal() {
            return;
        }

        let line_height = config.editor.line_height();
        let Some(start_vline) = screen_lines.lines.first() else {
            return;
        };
        let start_info = screen_lines.vline_info(*start_vline).unwrap();
        let start_line = start_info.rvline.line;

        let sticky_header_info = self.editor.sticky_header_info.get_untracked();
        let total_sticky_lines = sticky_header_info.sticky_lines.len();

        let paint_last_line = total_sticky_lines > 0
            && (sticky_header_info.last_sticky_should_scroll
                || sticky_header_info.y_diff != 0.0
                || start_line + total_sticky_lines - 1
                    != *sticky_header_info.sticky_lines.last().unwrap());

        let total_sticky_lines = if paint_last_line {
            total_sticky_lines
        } else {
            total_sticky_lines.saturating_sub(1)
        };

        if total_sticky_lines == 0 {
            return;
        }

        let scroll_offset = if sticky_header_info.last_sticky_should_scroll {
            sticky_header_info.y_diff
        } else {
            0.0
        };

        // Clear background

        let area_height = sticky_header_info
            .sticky_lines
            .iter()
            .copied()
            .map(|line| {
                let layout = self.editor.editor.text_layout(line);
                layout.line_count() * line_height
            })
            .sum::<usize>() as f64
            - scroll_offset;

        let sticky_area_rect = Size::new(viewport.x1, area_height)
            .to_rect()
            .with_origin(Point::new(0.0, viewport.y0))
            .inflate(10.0, 0.0);

        cx.fill(
            &sticky_area_rect,
            config.color(SparrowColor::SPARROW_DROPDOWN_SHADOW),
            3.0,
        );
        cx.fill(
            &sticky_area_rect,
            config.color(SparrowColor::EDITOR_STICKY_HEADER_BACKGROUND),
            0.0,
        );
        self.editor.sticky_header_info.get_untracked();
        // Paint lines
        let mut y_accum = 0.0;
        for (i, line) in sticky_header_info.sticky_lines.iter().copied().enumerate()
        {
            let y_diff = if i == total_sticky_lines - 1 {
                scroll_offset
            } else {
                0.0
            };

            let text_layout = self.editor.editor.text_layout(line);

            let text_height = (text_layout.line_count() * line_height) as f64;
            let height = text_height - y_diff;

            cx.save();

            let line_area_rect = Size::new(viewport.width(), height)
                .to_rect()
                .with_origin(Point::new(viewport.x0, viewport.y0 + y_accum));

            cx.clip(&line_area_rect);

            let y = viewport.y0 - y_diff + y_accum;
            cx.draw_text(&text_layout.text, Point::new(viewport.x0, y));

            y_accum += text_height;

            cx.restore();
        }
    }
}
