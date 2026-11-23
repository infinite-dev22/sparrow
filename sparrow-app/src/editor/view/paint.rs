//! Editor painting, including breakpoint highlights, search and inline guides.

use std::{cmp, collections::BTreeMap, sync::Arc};

use floem::{
    Renderer,
    context::PaintCx,
    kurbo::Stroke,
    peniko::{
        Color,
        kurbo::{Line, Point, Rect, Size},
    },
    reactive::{SignalGet, SignalWith},
    views::editor::{
        Editor, EditorStyle,
        view::{DiffSectionKind, EditorView as FloemEditorView, ScreenLines},
        visual_line::RVLine,
    },
};
use sparrow_core::{
    buffer::rope_text::RopeText,
    cursor::{CursorAffinity, CursorMode},
    selection::SelRegion,
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    debug::SparrowBreakpoint,
};

use super::{EditorView, changes_colors_all};

// Editor painting uses document coordinates; the scroll view applies the
// viewport translation. Gutter overlays apply that translation themselves.
fn breakpoint_backgrounds<'a>(
    lines: &'a ScreenLines,
    points: &'a BTreeMap<usize, SparrowBreakpoint>,
    viewport: Rect,
    line_height: f64,
) -> impl Iterator<Item = Rect> + 'a {
    lines.iter_lines_y().filter_map(move |(line, y)| {
        points
            .get(&line)
            .filter(|point| point.active)
            .map(|_| Rect::new(viewport.x0, y, viewport.x1, y + line_height))
    })
}

impl EditorView {
    fn paint_diff_sections(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
        config: &SparrowConfig,
    ) {
        let Some(diff_sections) = &screen_lines.diff_sections else {
            return;
        };
        for section in diff_sections.iter() {
            match section.kind {
                DiffSectionKind::NoCode => self.paint_diff_no_code(
                    cx,
                    viewport,
                    section.y_idx,
                    section.height,
                    config,
                ),
                DiffSectionKind::Added => {
                    cx.fill(
                        &Rect::ZERO
                            .with_size(Size::new(
                                viewport.width(),
                                (config.editor.line_height() * section.height)
                                    as f64,
                            ))
                            .with_origin(Point::new(
                                viewport.x0,
                                (section.y_idx * config.editor.line_height()) as f64,
                            )),
                        config
                            .color(SparrowColor::SOURCE_CONTROL_ADDED)
                            .multiply_alpha(0.2),
                        0.0,
                    );
                }
                DiffSectionKind::Removed => {
                    cx.fill(
                        &Rect::ZERO
                            .with_size(Size::new(
                                viewport.width(),
                                (config.editor.line_height() * section.height)
                                    as f64,
                            ))
                            .with_origin(Point::new(
                                viewport.x0,
                                (section.y_idx * config.editor.line_height()) as f64,
                            )),
                        config
                            .color(SparrowColor::SOURCE_CONTROL_REMOVED)
                            .multiply_alpha(0.2),
                        0.0,
                    );
                }
            }
        }
    }

    fn paint_diff_no_code(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        start_line: usize,
        height: usize,
        config: &SparrowConfig,
    ) {
        let line_height = config.editor.line_height();
        let height = (height * line_height) as f64;
        let y = (start_line * line_height) as f64;
        let y_end = y + height;

        if y_end < viewport.y0 || y > viewport.y1 {
            return;
        }

        let y = y.max(viewport.y0 - 10.0);
        let y_end = y_end.min(viewport.y1 + 10.0);
        let height = y_end - y;

        let start_x = viewport.x0.floor() as usize;
        let start_x = start_x - start_x % 8;

        for x in (start_x..viewport.x1.ceil() as usize + 1 + height.ceil() as usize)
            .step_by(8)
        {
            let p0 = if x as f64 > viewport.x1.ceil() {
                Point::new(viewport.x1.ceil(), y + (x as f64 - viewport.x1.ceil()))
            } else {
                Point::new(x as f64, y)
            };

            let height = if x as f64 - height < viewport.x0.floor() {
                x as f64 - viewport.x0.floor()
            } else {
                height
            };
            if height > 0.0 {
                let p1 = Point::new(x as f64 - height, y + height);
                cx.stroke(
                    &Line::new(p0, p1),
                    config.color(SparrowColor::EDITOR_DIM),
                    &Stroke::new(1.0),
                );
            }
        }
    }

    fn paint_current_line(
        &self,
        cx: &mut PaintCx,
        is_local: bool,
        screen_lines: &ScreenLines,
    ) {
        let e_data = self.editor.clone();
        let ed = e_data.editor.clone();
        let cursor = self.editor.cursor();
        let config = self.editor.common.config;

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let viewport = self.viewport.get_untracked();

        let current_line_color = ed.es.with_untracked(EditorStyle::current_line);

        let breakline = self.debug_breakline.get_untracked().and_then(
            |(breakline, breakline_path)| {
                if e_data
                    .doc()
                    .content
                    .with_untracked(|c| c.path() == Some(&breakline_path))
                {
                    Some(breakline)
                } else {
                    None
                }
            },
        );

        // TODO: check if this is correct
        if let Some(breakline) = breakline {
            if let Some(info) = screen_lines.info_for_line(breakline) {
                let rect = Rect::from_origin_size(
                    (viewport.x0, info.vline_y),
                    (viewport.width(), line_height),
                );

                cx.fill(
                    &rect,
                    config.color(SparrowColor::EDITOR_DEBUG_BREAK_LINE),
                    0.0,
                );
            }
        }

        cursor.with_untracked(|cursor| {
            let highlight_current_line = match cursor.mode {
                CursorMode::Normal(_) | CursorMode::Insert(_) => true,
                CursorMode::Visual { .. } => false,
            };

            if let Some(current_line_color) = current_line_color {
                // Highlight the current line
                if !is_local && highlight_current_line {
                    for (_, end) in cursor.regions_iter() {
                        // TODO: unsure if this is correct for wrapping lines
                        let rvline = ed.rvline_of_offset(end, cursor.affinity);

                        if let Some(info) = screen_lines
                            .info(rvline)
                            .filter(|_| Some(rvline.line) != breakline)
                        {
                            let rect = Rect::from_origin_size(
                                (viewport.x0, info.vline_y),
                                (viewport.width(), line_height),
                            );

                            cx.fill(&rect, current_line_color, 0.0);
                        }
                    }
                }
            }
        });
    }

    fn paint_find(&self, cx: &mut PaintCx, screen_lines: &ScreenLines) {
        let find_visual = self.editor.common.find.visual.get_untracked();
        if !find_visual && self.editor.on_screen_find.with_untracked(|f| !f.active) {
            return;
        }
        if screen_lines.lines.is_empty() {
            return;
        }

        let min_vline = *screen_lines.lines.first().unwrap();
        let max_vline = *screen_lines.lines.last().unwrap();
        let min_line = screen_lines.info(min_vline).unwrap().vline_info.rvline.line;
        let max_line = screen_lines.info(max_vline).unwrap().vline_info.rvline.line;

        let e_data = &self.editor;
        let ed = &e_data.editor;
        let doc = e_data.doc();

        let config = self.editor.common.config;
        let occurrences = doc.find_result.occurrences;

        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let color = config.color(SparrowColor::EDITOR_FOREGROUND);

        let start = ed.offset_of_line(min_line);
        let end = ed.offset_of_line(max_line + 1);

        // TODO: The selection rect creation logic for find is quite similar to the version
        // within insert cursor. It would be good to deduplicate it.
        if find_visual {
            doc.update_find();
            for region in occurrences.with_untracked(|selection| {
                selection.regions_in_range(start, end).to_vec()
            }) {
                self.paint_find_region(
                    cx,
                    ed,
                    &region,
                    color,
                    screen_lines,
                    line_height,
                );
            }
        }

        self.editor.on_screen_find.with_untracked(|find| {
            if find.active {
                for region in &find.regions {
                    self.paint_find_region(
                        cx,
                        ed,
                        region,
                        color,
                        screen_lines,
                        line_height,
                    );
                }
            }
        });
    }

    fn paint_document_highlights(
        &self,
        cx: &mut PaintCx,
        screen_lines: &ScreenLines,
    ) {
        let (color, line_height) =
            self.editor.common.config.with_untracked(|config| {
                (
                    // config.color(SparrowColor::EDITOR_FOREGROUND);
                    config.color(SparrowColor::ERROR_LENS_OTHER_FOREGROUND),
                    config.editor.line_height() as f64,
                )
            });

        let ed = &self.editor.editor;
        let doc = self.editor.doc();

        doc.highlights.with_untracked(move |highlights| {
            let Some(highlights) = highlights else {
                return;
            };
            for highlight in highlights {
                self.paint_find_region(
                    cx,
                    ed,
                    &highlight.into(),
                    color,
                    screen_lines,
                    line_height,
                );
            }
        });
    }

    fn paint_find_region(
        &self,
        cx: &mut PaintCx,
        ed: &Editor,
        region: &SelRegion,
        color: Color,
        screen_lines: &ScreenLines,
        line_height: f64,
    ) {
        let start = region.min();
        let end = region.max();

        // TODO(minor): the proper affinity here should probably be tracked by selregion
        let (start_rvline, start_col) =
            ed.rvline_col_of_offset(start, CursorAffinity::Forward);
        let (end_rvline, end_col) =
            ed.rvline_col_of_offset(end, CursorAffinity::Backward);

        for line_info in screen_lines.iter_line_info() {
            let rvline_info = line_info.vline_info;
            let rvline = rvline_info.rvline;
            let line = rvline.line;

            if rvline < start_rvline {
                continue;
            }

            if rvline > end_rvline {
                break;
            }

            let left_col = if rvline == start_rvline { start_col } else { 0 };
            let (right_col, _vline_end) = if rvline == end_rvline {
                let max_col = ed.last_col(rvline_info, true);
                (end_col.min(max_col), false)
            } else {
                (ed.last_col(rvline_info, true), true)
            };

            // TODO(minor): sel region should have the affinity of the start/end
            let x0 = ed
                .line_point_of_line_col(
                    line,
                    left_col,
                    CursorAffinity::Forward,
                    true,
                )
                .x;
            let x1 = ed
                .line_point_of_line_col(
                    line,
                    right_col,
                    CursorAffinity::Backward,
                    true,
                )
                .x;

            if !rvline_info.is_empty() && start != end && left_col != right_col {
                let rect = Size::new(x1 - x0, line_height)
                    .to_rect()
                    .with_origin(Point::new(x0, line_info.vline_y));
                cx.stroke(&rect, color, &Stroke::new(1.0));
            }
        }
    }

    fn paint_scroll_bar(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        is_local: bool,
        config: Arc<SparrowConfig>,
    ) {
        const BAR_WIDTH: f64 = 10.0;

        if is_local {
            return;
        }

        cx.fill(
            &Rect::ZERO
                .with_size(Size::new(1.0, viewport.height()))
                .with_origin(Point::new(
                    viewport.x0 + viewport.width() - BAR_WIDTH,
                    viewport.y0,
                ))
                .inflate(0.0, 10.0),
            config.color(SparrowColor::SPARROW_SCROLL_BAR),
            0.0,
        );

        if !self.editor.kind.get_untracked().is_normal() {
            return;
        }

        let doc = self.editor.doc();
        let total_len = doc.buffer.with_untracked(|buffer| buffer.last_line());
        let changes = doc.head_changes().get_untracked();
        let total_height = viewport.height();
        let total_width = viewport.width();
        let line_height = config.editor.line_height();
        let content_height = if config.editor.scroll_beyond_last_line {
            (total_len * line_height) as f64 + total_height - line_height as f64
        } else {
            (total_len * line_height) as f64
        };

        let colors = changes_colors_all(&config, &self.editor.editor, changes);
        for (y, height, _, color) in colors {
            let y = y / content_height * total_height;
            let height = ((height * line_height) as f64 / content_height
                * total_height)
                .max(3.0);
            let rect = Rect::ZERO.with_size(Size::new(3.0, height)).with_origin(
                Point::new(
                    viewport.x0 + total_width - BAR_WIDTH + 1.0,
                    y + viewport.y0,
                ),
            );
            cx.fill(&rect, color, 0.0);
        }
    }

    /// Paint a highlight around the characters at the given positions.
    fn paint_char_highlights(
        &self,
        cx: &mut PaintCx,
        screen_lines: &ScreenLines,
        highlight_line_cols: impl Iterator<Item = (RVLine, usize)>,
    ) {
        let editor = &self.editor.editor;
        let config = self.editor.common.config.get_untracked();
        let line_height = config.editor.line_height() as f64;

        for (rvline, col) in highlight_line_cols {
            // Is the given line on screen?
            if let Some(line_info) = screen_lines.info(rvline) {
                let x0 = editor
                    .line_point_of_line_col(
                        rvline.line,
                        col,
                        CursorAffinity::Forward,
                        true,
                    )
                    .x;
                let x1 = editor
                    .line_point_of_line_col(
                        rvline.line,
                        col + 1,
                        CursorAffinity::Backward,
                        true,
                    )
                    .x;

                let y0 = line_info.vline_y;
                let y1 = y0 + line_height;

                let rect = Rect::new(x0, y0, x1, y1);

                cx.stroke(
                    &rect,
                    config.color(SparrowColor::EDITOR_FOREGROUND),
                    &Stroke::new(1.0),
                );
            }
        }
    }

    /// Paint scope lines between `(start_rvline, start_line, start_col)` and
    /// `(end_rvline, end_line end_col)`.
    fn paint_scope_lines(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
        (start, start_col): (RVLine, usize),
        (end, end_col): (RVLine, usize),
    ) {
        let editor = &self.editor.editor;
        let doc = self.editor.doc();
        let config = self.editor.common.config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let brush = config.color(SparrowColor::EDITOR_FOREGROUND);

        if start == end {
            if let Some(line_info) = screen_lines.info(start) {
                // TODO: Due to line wrapping the y positions of these two spots could be different, do we need to change it?
                let x0 = editor
                    .line_point_of_line_col(
                        start.line,
                        start_col + 1,
                        CursorAffinity::Forward,
                        true,
                    )
                    .x;
                let x1 = editor
                    .line_point_of_line_col(
                        end.line,
                        end_col,
                        CursorAffinity::Backward,
                        true,
                    )
                    .x;

                if x0 < x1 {
                    let y = line_info.vline_y + line_height;

                    let p0 = Point::new(x0, y);
                    let p1 = Point::new(x1, y);
                    let line = Line::new(p0, p1);

                    cx.stroke(&line, brush, &Stroke::new(1.0));
                }
            }
        } else {
            // Are start_line and end_line on screen?
            let start_line_y = screen_lines
                .info(start)
                .map(|line_info| line_info.vline_y + line_height);
            let end_line_y = screen_lines
                .info(end)
                .map(|line_info| line_info.vline_y + line_height);

            // We only need to draw anything if start_line is on or before the visible section and
            // end_line is on or after the visible section.
            let y0 = start_line_y.or_else(|| {
                screen_lines
                    .lines
                    .first()
                    .is_some_and(|&first_vline| first_vline > start)
                    .then(|| viewport.min_y())
            });
            let y1 = end_line_y.or_else(|| {
                screen_lines
                    .lines
                    .last()
                    .is_some_and(|&last_line| last_line < end)
                    .then(|| viewport.max_y())
            });

            if let [Some(y0), Some(y1)] = [y0, y1] {
                let start_x = editor
                    .line_point_of_line_col(
                        start.line,
                        start_col + 1,
                        CursorAffinity::Forward,
                        true,
                    )
                    .x;
                let end_x = editor
                    .line_point_of_line_col(
                        end.line,
                        end_col,
                        CursorAffinity::Backward,
                        true,
                    )
                    .x;

                // TODO(minor): is this correct with line wrapping?
                // The vertical line should be drawn to the left of any non-whitespace characters
                // in the enclosed section.
                let min_text_x = doc.buffer.with_untracked(|buffer| {
                    ((start.line + 1)..=end.line)
                        .filter(|&line| !buffer.is_line_whitespace(line))
                        .map(|line| {
                            let non_blank_offset =
                                buffer.first_non_blank_character_on_line(line);
                            let (_, col) =
                                editor.offset_to_line_col(non_blank_offset);

                            editor
                                .line_point_of_line_col(
                                    line,
                                    col,
                                    CursorAffinity::Backward,
                                    true,
                                )
                                .x
                        })
                        .min_by(f64::total_cmp)
                });

                let min_x = min_text_x.map_or(start_x, |min_text_x| {
                    cmp::min_by(min_text_x, start_x, f64::total_cmp)
                });

                // Is start_line on screen, and is the vertical line to the left of the opening
                // bracket?
                if let Some(y) = start_line_y.filter(|_| start_x > min_x) {
                    let p0 = Point::new(min_x, y);
                    let p1 = Point::new(start_x, y);
                    let line = Line::new(p0, p1);

                    cx.stroke(&line, brush, &Stroke::new(1.0));
                }

                // Is end_line on screen, and is the vertical line to the left of the closing
                // bracket?
                if let Some(y) = end_line_y.filter(|_| end_x > min_x) {
                    let p0 = Point::new(min_x, y);
                    let p1 = Point::new(end_x, y);
                    let line = Line::new(p0, p1);

                    cx.stroke(&line, brush, &Stroke::new(1.0));
                }

                let p0 = Point::new(min_x, y0);
                let p1 = Point::new(min_x, y1);
                let line = Line::new(p0, p1);

                cx.stroke(&line, brush, &Stroke::new(1.0));
            }
        }
    }

    /// Paint enclosing bracket highlights and scope lines if the corresponding settings are
    /// enabled.
    fn paint_bracket_highlights_scope_lines(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
    ) {
        let config = self.editor.common.config.get_untracked();

        if config.editor.highlight_matching_brackets
            || config.editor.highlight_scope_lines
        {
            let e_data = &self.editor;
            let ed = &e_data.editor;
            let offset = ed.cursor.with_untracked(|cursor| cursor.mode.offset());

            let bracket_offsets = e_data
                .doc_signal()
                .with_untracked(|doc| doc.find_enclosing_brackets(offset))
                .map(|(start, end)| [start, end]);

            let bracket_line_cols = bracket_offsets.map(|bracket_offsets| {
                bracket_offsets.map(|offset| {
                    let (rvline, col) =
                        ed.rvline_col_of_offset(offset, CursorAffinity::Forward);
                    (rvline, col)
                })
            });

            if config.editor.highlight_matching_brackets {
                self.paint_char_highlights(
                    cx,
                    screen_lines,
                    bracket_line_cols.into_iter().flatten(),
                );
            }

            if config.editor.highlight_scope_lines {
                if let Some([start_line_col, end_line_col]) = bracket_line_cols {
                    self.paint_scope_lines(
                        cx,
                        viewport,
                        screen_lines,
                        start_line_col,
                        end_line_col,
                    );
                }
            }
        }
    }

    fn paint_closing_label_backgrounds(
        &self,
        cx: &mut PaintCx,
        screen_lines: &ScreenLines,
        config: &SparrowConfig,
    ) {
        if !config.editor.enable_inlay_hints {
            return;
        }
        let doc = self.editor.doc();
        let Some(labels) = doc.closing_labels.get_untracked() else {
            return;
        };
        for (line, y) in screen_lines.iter_lines_y() {
            let layout = self.editor.editor.text_layout(line);
            for (shift, size, col, phantom) in layout.phantom_text.offset_size_iter()
            {
                if !labels.iter().any(|label| {
                    label.range.end.line as usize == line
                        && phantom.text == format!(" {} ", label.label)
                }) {
                    continue;
                }
                for range in crate::doc::extra_styles_for_range(
                    &layout.text,
                    col + shift,
                    col + shift + size,
                    None,
                    None,
                    None,
                ) {
                    let rect = Rect::new(
                        range.x,
                        y + range.y - 1.0,
                        range.x + range.width.unwrap_or_default(),
                        y + range.y + range.height + 1.0,
                    );
                    cx.fill(
                        &rect.to_rounded_rect(3.0),
                        config.color(SparrowColor::INLAY_HINT_BACKGROUND),
                        0.0,
                    );
                }
            }
        }
    }

    fn paint_flutter_widget_guides(
        &self,
        cx: &mut PaintCx,
        viewport: Rect,
        screen_lines: &ScreenLines,
        config: &SparrowConfig,
    ) {
        let e_data = &self.editor;
        let doc = e_data.doc();
        let path = doc.content.with_untracked(|c| c.path().cloned());
        let Some(path) = path else { return };
        if !crate::dart::is_dart_path(&path) {
            return;
        }

        let editor = &e_data.editor;
        let line_height = config.editor.line_height() as f64;
        let brush = config.color(SparrowColor::EDITOR_INDENT_GUIDE);

        let content = doc.buffer.with_untracked(|b| b.to_string());
        let guides = crate::dart::extract_widget_guides(&content);

        for guide in guides {
            let start_rvline = editor.rvline_of_line(guide.start_line);
            let end_rvline = editor.rvline_of_line(guide.end_line);

            let start_info = screen_lines.info(start_rvline);
            let end_info = screen_lines.info(end_rvline);

            if start_info.is_none() && end_info.is_none() {
                continue;
            }

            let y0 = start_info
                .as_ref()
                .map(|i| i.vline_y + line_height)
                .unwrap_or(viewport.min_y());
            let y1 = end_info
                .as_ref()
                .map(|i| i.vline_y)
                .unwrap_or(viewport.max_y());

            if y1 <= y0 {
                continue;
            }

            let start_pt = editor.line_point_of_line_col(
                guide.start_line,
                guide.start_col,
                CursorAffinity::Forward,
                true,
            );

            let x = start_pt.x + 4.0;
            let line = Line::new(Point::new(x, y0), Point::new(x, y1));
            cx.stroke(&line, brush, &Stroke::new(1.0));

            // Small horizontal tick / elbow pointing towards closing paren
            if let Some(end_info) = end_info {
                let end_y = end_info.vline_y + line_height / 2.0;
                let elbow =
                    Line::new(Point::new(x, end_y), Point::new(x + 6.0, end_y));
                cx.stroke(&elbow, brush, &Stroke::new(1.0));
            }
        }
    }

    pub(super) fn paint_document(&mut self, cx: &mut PaintCx) {
        let viewport = self.viewport.get_untracked();
        let e_data = &self.editor;
        let ed = &e_data.editor;
        let config = e_data.common.config.get_untracked();
        let doc = e_data.doc();
        let is_local = doc.content.with_untracked(|content| content.is_local());
        let find_focus = self.editor.find_focus;
        let is_active =
            self.is_active.get_untracked() && !find_focus.get_untracked();

        // We repeatedly get the screen lines because we don't currently carefully manage the
        // paint functions to avoid potentially needing to recompute them, which could *maybe*
        // make them invalid.
        // TODO: One way to get around the above issue would be to more careful, since we
        // technically don't need to stop it from *recomputing* just stop any possible changes, but
        // avoiding recomputation seems easiest/clearest.
        // I expect that most/all of the paint functions could restrict themselves to only what is
        // within the active screen lines without issue.
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_current_line(cx, is_local, &screen_lines);
        if let Some(path) = doc
            .content
            .with_untracked(|content| content.path().cloned())
        {
            e_data.common.breakpoints.with_untracked(|all| {
                if let Some(points) = all.get(&path) {
                    for rect in breakpoint_backgrounds(
                        &screen_lines,
                        points,
                        viewport,
                        config.editor.line_height() as f64,
                    ) {
                        cx.fill(
                            &rect,
                            config
                                .color(SparrowColor::DEBUG_BREAKPOINT)
                                .with_alpha(0.18),
                            0.0,
                        );
                    }
                }
            });
        }
        FloemEditorView::paint_selection(cx, ed, &screen_lines);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_diff_sections(cx, viewport, &screen_lines, &config);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_find(cx, &screen_lines);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_document_highlights(cx, &screen_lines);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_bracket_highlights_scope_lines(cx, viewport, &screen_lines);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_flutter_widget_guides(cx, viewport, &screen_lines, &config);
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_closing_label_backgrounds(cx, &screen_lines, &config);
        FloemEditorView::paint_text(
            cx,
            None,
            ed,
            viewport,
            is_active,
            &screen_lines,
        );
        let screen_lines = ed.screen_lines.get_untracked();
        self.paint_sticky_headers(cx, viewport, &screen_lines);
        self.paint_scroll_bar(cx, viewport, is_local, config);
    }
}

#[cfg(test)]
mod breakpoint_background_tests {
    use super::*;
    use floem::reactive::Scope;
    use floem::views::editor::{view::LineInfo, visual_line::VLineInfo};
    use std::{collections::HashMap, rc::Rc};

    #[test]
    fn breakpoint_background_stays_on_its_document_line_when_scrolled() {
        let scope = Scope::new();
        let rvline = RVLine::new(20, 0);
        let points = BTreeMap::from([(
            20,
            SparrowBreakpoint {
                id: None,
                verified: false,
                message: None,
                line: 20,
                offset: 0,
                dap_line: None,
                active: true,
            },
        )]);
        for scroll_y in [0.0, 100.0, 200.5, 415.0] {
            let viewport = Rect::new(30.0, scroll_y, 830.0, scroll_y + 600.0);
            let mut lines = ScreenLines::new(scope, viewport);
            lines.lines = Rc::new(vec![rvline]);
            lines.info = Rc::new(HashMap::from([(
                rvline,
                LineInfo {
                    y: 420.0 - scroll_y,
                    vline_y: 420.0 - scroll_y,
                    vline_info: VLineInfo::new(0..1, rvline, 1, ()),
                },
            )]));
            let rects = breakpoint_backgrounds(&lines, &points, viewport, 21.0)
                .collect::<Vec<_>>();
            assert_eq!(rects, vec![Rect::new(30.0, 420.0, 830.0, 441.0)]);
            // After the scroll view's translation the fill and text share a top edge.
            assert_eq!(
                rects[0].y0 - scroll_y,
                lines.info(rvline).unwrap().y - scroll_y
            );
            let mut inactive = points.clone();
            inactive.get_mut(&20).unwrap().active = false;
            assert_eq!(
                breakpoint_backgrounds(&lines, &inactive, viewport, 21.0).count(),
                0
            );
        }
        scope.dispose();
    }
}
