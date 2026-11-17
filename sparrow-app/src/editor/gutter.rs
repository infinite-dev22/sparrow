use floem::{
    Renderer, View, ViewId,
    context::PaintCx,
    peniko::kurbo::{Point, Rect, Size},
    reactive::{Memo, SignalGet, SignalWith},
    text::{Attrs, AttrsList, FamilyOwned, TextLayout},
};
use im::HashMap;
use serde::{Deserialize, Serialize};
use sparrow_core::{buffer::rope_text::RopeText, mode::Mode};

use super::{EditorData, view::changes_colors_screen};
use crate::config::{SparrowConfig, color::SparrowColor};

pub struct EditorGutterView {
    id: ViewId,
    editor: EditorData,
    width: f64,
    gutter_padding_right: Memo<f32>,
    lanes: Memo<GutterLanes>,
    previews: Memo<(Vec<crate::dart::ColorSwatch>, Vec<crate::dart::IconSwatch>)>,
}

pub(crate) fn editor_gutter_view(
    editor: EditorData,
    gutter_padding_right: Memo<f32>,
    lanes: Memo<GutterLanes>,
    previews: Memo<(Vec<crate::dart::ColorSwatch>, Vec<crate::dart::IconSwatch>)>,
) -> EditorGutterView {
    let id = ViewId::new();
    let bookmarks = editor.common.bookmarks;
    floem::reactive::create_effect(move |_| {
        bookmarks.get();
        lanes.get();
        previews.get();
        id.request_paint();
    });

    EditorGutterView {
        id,
        editor,
        width: 0.0,
        gutter_padding_right,
        lanes,
        previews,
    }
}

impl EditorGutterView {
    fn paint_head_changes(
        &self,
        cx: &mut PaintCx,
        e_data: &EditorData,
        viewport: Rect,
        is_normal: bool,
        config: &SparrowConfig,
    ) {
        if !is_normal {
            return;
        }

        let changes = e_data.doc().head_changes().get_untracked();
        let line_height = config.editor.line_height() as f64;

        let changes = changes_colors_screen(config, &e_data.editor, changes);
        for (y, height, removed, color) in changes {
            let height = if removed {
                10.0
            } else {
                height as f64 * line_height
            };
            let mut y = y - viewport.y0;
            if removed {
                y -= 5.0;
            }
            cx.fill(
                &Size::new(3.0, height)
                    .to_rect()
                    .with_origin(Point::new(self.width - 3.0, y)),
                color,
                0.0,
            )
        }
    }

    fn paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        is_normal: bool,
        config: &SparrowConfig,
    ) {
        if !is_normal {
            return;
        }

        if !config.editor.sticky_header {
            return;
        }
        let sticky_header_height = self.editor.sticky_header_height.get_untracked();
        if sticky_header_height == 0.0 {
            return;
        }

        let sticky_area_rect =
            Size::new(self.width + 25.0 + 30.0, sticky_header_height)
                .to_rect()
                .with_origin(Point::new(-25.0, 0.0))
                .inflate(25.0, 0.0);
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
    }

    fn paint_flutter_swatches(
        &self,
        cx: &mut floem::context::PaintCx,
        viewport: Rect,
        is_normal: bool,
        config: &SparrowConfig,
    ) {
        if !is_normal {
            return;
        }

        let doc = self.editor.doc();
        let path = doc.content.with_untracked(|c| c.path().cloned());
        let Some(path) = path else { return };
        let (colors, icons) = self.previews.get_untracked();
        let lanes = self.lanes.get_untracked();
        let origin = self.width - self.gutter_padding_right.get_untracked() as f64;
        let line_height = config.editor.line_height() as f64;
        let bookmarks =
            self.editor.common.bookmarks.with_untracked(|items| {
                items.get(&path).cloned().unwrap_or_default()
            });
        self.editor.screen_lines().with_untracked(|lines| {
            for (line, y) in lines.iter_lines_y() {
                let top = y - viewport.y0;
                if bookmarks.contains(&line) {
                    let x = origin + lanes.bookmark as f64 + 2.0;
                    let y = top + (line_height - 13.0) / 2.0;
                    let mut shape = floem::peniko::kurbo::BezPath::new();
                    shape.move_to((x, y));
                    shape.line_to((x + 10.0, y));
                    shape.line_to((x + 10.0, y + 13.0));
                    shape.line_to((x + 5.0, y + 9.0));
                    shape.line_to((x, y + 13.0));
                    shape.close_path();
                    cx.fill(
                        &shape,
                        floem::peniko::Color::from_rgb8(230, 189, 96),
                        0.0,
                    );
                }
                for (slot, swatch) in
                    colors.iter().filter(|c| c.line == line).enumerate()
                {
                    let rect = Rect::from_origin_size(
                        (
                            origin + lanes.colors as f64 + slot as f64 * 14.0 + 2.0,
                            top + (line_height - 10.0) / 2.0,
                        ),
                        (10.0, 10.0),
                    );
                    let [r, g, b, a] = swatch.rgba;
                    cx.fill(
                        &rect,
                        floem::peniko::Color::from_rgba8(r, g, b, a),
                        1.0,
                    );
                    cx.stroke(
                        &rect,
                        config.color(SparrowColor::EDITOR_DIM),
                        &floem::peniko::kurbo::Stroke::new(1.0),
                    );
                }
                for (slot, icon) in
                    icons.iter().filter(|i| i.line == line).enumerate()
                {
                    if let Some(glyph) = icon.codepoint.and_then(char::from_u32) {
                        let family = FamilyOwned::parse_list(if icon.is_cupertino {
                            "CupertinoIcons"
                        } else {
                            "Material Icons"
                        })
                        .collect::<Vec<_>>();
                        let mut layout = TextLayout::new();
                        layout.set_text(
                            &glyph.to_string(),
                            AttrsList::new(
                                Attrs::new().family(&family).font_size(14.0).color(
                                    config.color(SparrowColor::EDITOR_FOREGROUND),
                                ),
                            ),
                            None,
                        );
                        cx.draw_text(
                            &layout,
                            Point::new(
                                origin + lanes.icons as f64 + slot as f64 * 16.0,
                                top + (line_height - layout.size().height) / 2.0,
                            ),
                        );
                    }
                }
            }
        });
    }
}

impl View for EditorGutterView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn compute_layout(
        &mut self,
        _cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<floem::peniko::kurbo::Rect> {
        if let Some(width) = self.id.get_layout().map(|l| l.size.width as f64) {
            self.width = width;
        }
        None
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        let viewport = self.editor.viewport().get_untracked();
        let cursor = self.editor.cursor();
        let screen_lines = self.editor.screen_lines();
        let config = self.editor.common.config;

        let kind_is_normal =
            self.editor.kind.with_untracked(|kind| kind.is_normal());
        let (offset, mode) = cursor.with_untracked(|c| (c.offset(), c.get_mode()));
        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let last_line = self.editor.editor.last_line();
        let current_line = self
            .editor
            .doc()
            .buffer
            .with_untracked(|buffer| buffer.line_of_offset(offset));

        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.editor.font_family).collect();
        let attrs = Attrs::new()
            .family(&family)
            .color(config.color(SparrowColor::EDITOR_DIM))
            .font_size(config.editor.font_size() as f32);
        let attrs_list = AttrsList::new(attrs.clone());
        let current_line_attrs_list = AttrsList::new(
            attrs
                .clone()
                .color(config.color(SparrowColor::EDITOR_FOREGROUND)),
        );
        let show_relative = config.core.modal
            && config.editor.modal_mode_relative_line_numbers
            && mode != Mode::Insert
            && kind_is_normal;

        screen_lines.with_untracked(|screen_lines| {
            for (line, y) in screen_lines.iter_lines_y() {
                // If it ends up outside the bounds of the file, stop trying to display line numbers
                if line > last_line {
                    break;
                }

                let text = if show_relative {
                    if line == current_line {
                        line + 1
                    } else {
                        line.abs_diff(current_line)
                    }
                } else {
                    line + 1
                }
                .to_string();

                let mut text_layout = TextLayout::new();
                if line == current_line {
                    text_layout.set_text(
                        &text,
                        current_line_attrs_list.clone(),
                        None,
                    );
                } else {
                    text_layout.set_text(&text, attrs_list.clone(), None);
                }
                let size = text_layout.size();
                let height = size.height;

                cx.draw_text(
                    &text_layout,
                    Point::new(
                        (self.width
                            - size.width
                            - self.gutter_padding_right.get_untracked() as f64)
                            .max(0.0),
                        y + (line_height - height) / 2.0 - viewport.y0,
                    ),
                );
            }
        });

        self.paint_head_changes(cx, &self.editor, viewport, kind_is_normal, &config);
        self.paint_flutter_swatches(cx, viewport, kind_is_normal, &config);
        self.paint_sticky_headers(cx, kind_is_normal, &config);
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "Editor Gutter".into()
    }
}

#[derive(Default, Clone)]
pub struct FoldingRanges(pub Vec<FoldingRange>);

#[derive(Default, Clone)]
pub struct FoldedRanges(pub Vec<FoldedRange>);

impl FoldingRanges {
    pub fn get_folded_range(&self) -> FoldedRanges {
        let mut range = Vec::new();
        let mut limit_line = 0;
        for item in &self.0 {
            if item.start.line < limit_line && limit_line > 0 {
                continue;
            }
            if item.status.is_folded() {
                range.push(crate::editor::gutter::FoldedRange {
                    start: item.start,
                    end: item.end,
                });
                limit_line = item.end.line;
            }
        }

        FoldedRanges(range)
    }
    pub fn to_display_items(&self) -> Vec<FoldingDisplayItem> {
        let mut folded = HashMap::new();
        let mut unfold_start: HashMap<u32, FoldingDisplayItem> = HashMap::new();
        let mut unfold_end = HashMap::new();
        let mut limit_line = 0;
        for item in &self.0 {
            if item.start.line < limit_line && limit_line > 0 {
                continue;
            }
            match item.status {
                FoldingRangeStatus::Fold => {
                    folded.insert(
                        item.start.line,
                        FoldingDisplayItem::Folded(item.start),
                    );
                    limit_line = item.end.line;
                }
                FoldingRangeStatus::Unfold => {
                    unfold_start.insert(
                        item.start.line,
                        FoldingDisplayItem::UnfoldStart(item.start),
                    );
                    unfold_end.insert(
                        item.end.line,
                        FoldingDisplayItem::UnfoldEnd(item.end),
                    );
                    limit_line = 0;
                }
            };
        }
        for (key, val) in unfold_end {
            unfold_start.insert(key, val);
        }
        for (key, val) in folded {
            unfold_start.insert(key, val);
        }

        unfold_start.into_iter().map(|x| x.1).collect()
    }
}

impl FoldedRanges {
    pub fn contain_line(&self, start_index: usize, line: u32) -> (bool, usize) {
        if start_index >= self.0.len() {
            return (false, start_index);
        }
        let mut last_index = start_index;
        for range in self.0[start_index..].iter() {
            if range.start.line >= line {
                return (false, last_index);
            } else if range.start.line < line && range.end.line > line {
                return (true, last_index);
            } else if range.end.line < line {
                last_index += 1;
            }
        }
        (false, last_index)
    }
}

#[derive(Debug, Clone)]
pub struct FoldedRange {
    pub start: FoldingPosition,
    pub end: FoldingPosition,
}

#[derive(Debug, Clone)]
pub struct FoldingRange {
    pub start: FoldingPosition,
    pub end: FoldingPosition,
    pub status: FoldingRangeStatus,
    pub collapsed_text: Option<String>,
}

impl FoldingRange {
    pub fn from_lsp(value: lsp_types::FoldingRange) -> Self {
        let lsp_types::FoldingRange {
            start_line,
            start_character,
            end_line,
            end_character,
            collapsed_text,
            ..
        } = value;
        let status = FoldingRangeStatus::Unfold;
        Self {
            start: FoldingPosition {
                line: start_line,
                character: start_character,
                // kind: kind.clone().map(|x| FoldingRangeKind::from(x)),
            },
            end: FoldingPosition {
                line: end_line,
                character: end_character,
                // kind: kind.map(|x| FoldingRangeKind::from(x)),
            },
            status,
            collapsed_text,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Copy)]
pub struct FoldingPosition {
    pub line: u32,
    pub character: Option<u32>,
    // pub kind: Option<FoldingRangeKind>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub enum FoldingRangeStatus {
    Fold,
    #[default]
    Unfold,
}

impl FoldingRangeStatus {
    pub fn click(&mut self) {
        // match self {
        //     FoldingRangeStatus::Fold => {
        //         *self = FoldingRangeStatus::Unfold;
        //     }
        //     FoldingRangeStatus::Unfold => {
        //         *self = FoldingRangeStatus::Fold;
        //     }
        // }
    }
    pub fn is_folded(&self) -> bool {
        *self == Self::Fold
    }
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum FoldingDisplayItem {
    UnfoldStart(FoldingPosition),
    Folded(FoldingPosition),
    UnfoldEnd(FoldingPosition),
}

impl FoldingDisplayItem {
    pub fn position(&self) -> FoldingPosition {
        match self {
            FoldingDisplayItem::UnfoldStart(x) => *x,
            FoldingDisplayItem::Folded(x) => *x,
            FoldingDisplayItem::UnfoldEnd(x) => *x,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Deserialize, Serialize, Clone, Hash, Copy)]
pub enum FoldingRangeKind {
    Comment,
    Imports,
    Region,
}

impl From<lsp_types::FoldingRangeKind> for FoldingRangeKind {
    fn from(value: lsp_types::FoldingRangeKind) -> Self {
        match value {
            lsp_types::FoldingRangeKind::Comment => FoldingRangeKind::Comment,
            lsp_types::FoldingRangeKind::Imports => FoldingRangeKind::Imports,
            lsp_types::FoldingRangeKind::Region => FoldingRangeKind::Region,
        }
    }
}

/// Positions relative to the right edge of the line-number column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GutterLanes {
    pub breakpoint: f32,
    pub bookmark: f32,
    pub colors: f32,
    pub icons: f32,
    pub run: f32,
    pub padding: f32,
}
impl GutterLanes {
    pub fn new(
        colors: usize,
        icons: usize,
        bookmarks: bool,
        run: bool,
        tests: bool,
    ) -> Self {
        let breakpoint = 4.0;
        let bookmark = breakpoint + 24.0;
        let colors_x = bookmark + if bookmarks { 14.0 } else { 0.0 };
        let icons_x = colors_x + colors as f32 * 14.0;
        let run_x = icons_x + icons as f32 * 16.0;
        let padding = run_x
            + if run { 24.0 } else { 0.0 }
            + if tests { 48.0 } else { 0.0 }
            + 28.0;
        Self {
            breakpoint,
            bookmark,
            colors: colors_x,
            icons: icons_x,
            run: run_x,
            padding,
        }
    }
}
#[cfg(test)]
mod lane_tests {
    use super::*;
    #[test]
    fn simultaneous_decorations_have_independent_nonoverlapping_space() {
        let minimal = GutterLanes::new(0, 0, false, false, false);
        let full = GutterLanes::new(2, 2, true, true, true);
        assert_eq!(full.colors, full.bookmark + 14.0);
        assert_eq!(full.icons, full.colors + 28.0);
        assert_eq!(full.run, full.icons + 32.0);
        assert!(full.padding > minimal.padding + 100.0);
    }
}
