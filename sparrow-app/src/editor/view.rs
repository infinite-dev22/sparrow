use std::{path::PathBuf, sync::Arc};

use floem::{
    View, ViewId,
    action::{set_ime_allowed, set_ime_cursor_area},
    context::{PaintCx, StyleCx},
    event::{Event, EventListener, EventPropagation},
    peniko::kurbo::{Rect, Size},
    prelude::SignalTrack,
    reactive::{
        Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith,
        create_effect, create_memo,
    },
    style::{CursorColor, Style, TextColor},
    taffy::prelude::NodeId,
    views::{
        Decorators,
        editor::{
            CurrentLineColor, CursorSurroundingLines, EditorStyle, IndentGuideColor,
            IndentStyleProp, Modal, ModalRelativeLine, PhantomColor,
            PlaceholderColor, PreeditUnderlineColor, RenderWhitespaceProp,
            ScrollBeyondLastLine, SelectionColor, ShowIndentGuide, SmartTab,
            VisibleWhitespaceColor, WrapProp, text::WrapMethod,
            view::EditorViewClass,
        },
    },
};
use sparrow_core::buffer::Buffer;

use super::{DocSignal, EditorData};
use crate::{
    config::{SparrowConfig, color::SparrowColor, editor::WrapStyle},
    doc::DocContent,
};

mod breadcrumbs;
mod container;
mod diff_markers;
mod find;
mod gutter;
mod inspection;
mod paint;
mod sticky_header;

pub use container::editor_container_view;
pub use diff_markers::{changes_colors_all, changes_colors_screen};
pub(crate) use gutter::toggle_gutter_breakpoint;
pub use sticky_header::StickyHeaderInfo;

use sticky_header::get_sticky_header_info;

fn editor_wrap(config: &SparrowConfig) -> WrapMethod {
    /// Minimum width that we'll allow the view to be wrapped at.
    const MIN_WRAPPED_WIDTH: f32 = 100.0;

    match config.editor.wrap_style {
        WrapStyle::None => WrapMethod::None,
        WrapStyle::EditorWidth => WrapMethod::EditorWidth,
        WrapStyle::WrapWidth => WrapMethod::WrapWidth {
            width: (config.editor.wrap_width as f32).max(MIN_WRAPPED_WIDTH),
        },
    }
}

pub fn editor_style(
    config: ReadSignal<Arc<SparrowConfig>>,
    doc: DocSignal,
    s: Style,
) -> Style {
    let config = config.get();
    let doc = doc.get();

    s.set(
        IndentStyleProp,
        doc.buffer.with_untracked(Buffer::indent_style),
    )
    .set(CursorColor, config.color(SparrowColor::EDITOR_CARET))
    .set(SelectionColor, config.color(SparrowColor::EDITOR_SELECTION))
    .set(
        CurrentLineColor,
        config.color(SparrowColor::EDITOR_CURRENT_LINE),
    )
    .set(
        VisibleWhitespaceColor,
        config.color(SparrowColor::EDITOR_VISIBLE_WHITESPACE),
    )
    .set(
        IndentGuideColor,
        config.color(SparrowColor::EDITOR_INDENT_GUIDE),
    )
    .set(ScrollBeyondLastLine, config.editor.scroll_beyond_last_line)
    .color(config.color(SparrowColor::EDITOR_FOREGROUND))
    .set(TextColor, config.color(SparrowColor::EDITOR_FOREGROUND))
    .set(PhantomColor, config.color(SparrowColor::EDITOR_DIM))
    .set(PlaceholderColor, config.color(SparrowColor::EDITOR_DIM))
    .set(
        PreeditUnderlineColor,
        config.color(SparrowColor::EDITOR_FOREGROUND),
    )
    .set(ShowIndentGuide, config.editor.show_indent_guide)
    .set(Modal, config.core.modal)
    .set(
        ModalRelativeLine,
        config.editor.modal_mode_relative_line_numbers,
    )
    .set(SmartTab, config.editor.smart_tab)
    .set(WrapProp, editor_wrap(&config))
    .set(
        CursorSurroundingLines,
        config.editor.cursor_surrounding_lines,
    )
    .set(RenderWhitespaceProp, config.editor.render_whitespace)
}

pub struct EditorView {
    id: ViewId,
    editor: EditorData,
    is_active: Memo<bool>,
    inner_node: Option<NodeId>,
    viewport: RwSignal<Rect>,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
}

pub fn editor_view(
    e_data: EditorData,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> EditorView {
    let id = ViewId::new();
    let is_active = create_memo(move |_| is_active(true));

    let viewport = e_data.viewport();

    let doc = e_data.doc_signal();
    let view_kind = e_data.kind;
    let screen_lines = e_data.screen_lines();
    create_effect(move |_| {
        doc.track();
        view_kind.track();
        id.request_layout();
    });

    let hide_cursor = e_data.common.window_common.hide_cursor;
    create_effect(move |_| {
        hide_cursor.track();
        let occurrences = doc.with(|doc| doc.find_result.occurrences);
        occurrences.track();
        id.request_paint();
    });

    create_effect(move |last_rev| {
        let buffer = doc.with(|doc| doc.buffer);
        let rev = buffer.with(|buffer| buffer.rev());
        if last_rev == Some(rev) {
            return rev;
        }
        id.request_layout();
        rev
    });

    let config = e_data.common.config;
    let sticky_header_height_signal = e_data.sticky_header_height;
    let editor2 = e_data.clone();
    create_effect(move |last_rev| {
        let config = config.get();
        if !config.editor.sticky_header {
            return (DocContent::Local, 0, 0, Rect::ZERO, 0, None);
        }

        let doc = doc.get();
        let rect = viewport.get();
        let (screen_lines_len, screen_lines_first) = screen_lines
            .with(|lines| (lines.lines.len(), lines.lines.first().copied()));
        let rev = (
            doc.content.get(),
            doc.buffer.with(|b| b.rev()),
            doc.cache_rev.get(),
            rect,
            screen_lines_len,
            screen_lines_first,
        );
        if last_rev.as_ref() == Some(&rev) {
            return rev;
        }

        let sticky_header_info = get_sticky_header_info(
            &editor2,
            viewport,
            sticky_header_height_signal,
            &config,
        );

        id.update_state(sticky_header_info);

        rev
    });

    create_effect({
        let editor_cursor = e_data.cursor();
        move |_| {
            let offset = editor_cursor.get().offset();
            doc.with(|doc| doc.update_highlights(offset));
        }
    });

    create_effect(move |_| {
        let highlights = doc.with(|doc| doc.highlights);
        highlights.track();
        id.request_paint();
    });

    let ed1 = e_data.editor.clone();
    let ed2 = ed1.clone();
    let ed3 = ed1.clone();

    let editor_window_origin = e_data.window_origin();
    let cursor = e_data.cursor();
    let find_focus = e_data.find_focus;
    let ime_allowed = e_data.common.window_common.ime_allowed;
    let editor_viewport = e_data.viewport();
    let editor_cursor = e_data.cursor();
    create_effect(move |_| {
        let active = is_active.get();
        if active && !find_focus.get() {
            if !cursor.with(|c| c.is_insert()) {
                if ime_allowed.get_untracked() {
                    ime_allowed.set(false);
                    set_ime_allowed(false);
                }
            } else {
                if !ime_allowed.get_untracked() {
                    ime_allowed.set(true);
                    set_ime_allowed(true);
                }
                let (offset, affinity) = cursor.with(|c| (c.offset(), c.affinity));
                let (_, point_below) = ed1.points_of_offset(offset, affinity);
                let window_origin = editor_window_origin.get();
                let viewport = editor_viewport.get();
                let pos = window_origin
                    + (point_below.x - viewport.x0, point_below.y - viewport.y0);
                set_ime_cursor_area(pos, Size::new(800.0, 600.0));
            }
        }
    });

    let doc = e_data.doc_signal();
    EditorView {
        id,
        editor: e_data,
        is_active,
        inner_node: None,
        viewport,
        debug_breakline,
    }
    .on_event(EventListener::ImePreedit, move |event| {
        if !is_active.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImePreedit { text, cursor } = event {
            if text.is_empty() {
                ed2.clear_preedit();
            } else {
                let offset = editor_cursor.with_untracked(|c| c.offset());
                ed2.set_preedit(text.clone(), *cursor, offset);
            }
        }
        EventPropagation::Stop
    })
    .on_event(EventListener::ImeCommit, move |event| {
        if !is_active.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImeCommit(text) = event {
            ed3.clear_preedit();
            ed3.receive_char(text);
        }
        EventPropagation::Stop
    })
    .class(EditorViewClass)
    .style(move |s| editor_style(config, doc, s))
}

impl View for EditorView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn style_pass(&mut self, cx: &mut StyleCx<'_>) {
        let editor = &self.editor.editor;
        if editor.es.try_update(|s| s.read(cx)).unwrap() {
            editor.floem_style_id.update(|val| *val += 1);
            cx.app_state_mut().request_paint(self.id());
        }
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "Editor View".into()
    }

    fn update(
        &mut self,
        _cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) {
        if let Ok(state) = state.downcast() {
            self.editor.sticky_header_info.set(*state);
            self.id.request_layout();
        }
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::NodeId {
        cx.layout_node(self.id, true, |_cx| {
            if self.inner_node.is_none() {
                self.inner_node = Some(self.id.new_taffy_node());
            }

            let e_data = &self.editor;
            let editor = &e_data.editor;

            let viewport_size = self.viewport.get_untracked().size();

            let screen_lines = e_data.screen_lines().get_untracked();
            for (line, _) in screen_lines.iter_lines_y() {
                // fill in text layout cache so that max width is correct.
                editor.text_layout(line);
            }

            let inner_node = self.inner_node.unwrap();

            let config = self.editor.common.config.get_untracked();
            let line_height = config.editor.line_height() as f64;

            let is_local = e_data.doc().content.with_untracked(|c| c.is_local());

            let width = editor.max_line_width() + 10.0;
            let width = if !is_local {
                width.max(viewport_size.width)
            } else {
                width
            };
            let last_vline = editor.last_vline().get();
            let last_vline = e_data.visual_line(last_vline);
            let last_line_height = line_height * (last_vline + 1) as f64;
            let height = last_line_height.max(line_height);
            let height = if !is_local {
                height.max(viewport_size.height)
            } else {
                height
            };

            let margin_bottom = if !is_local
                && editor
                    .es
                    .with_untracked(EditorStyle::scroll_beyond_last_line)
            {
                viewport_size.height.min(last_line_height) - line_height
            } else {
                0.0
            };

            let style = Style::new()
                .width(width)
                .height(height)
                .margin_bottom(margin_bottom)
                .to_taffy_style();
            self.id.set_taffy_style(inner_node, style);

            vec![inner_node]
        })
    }

    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<Rect> {
        let viewport = cx.current_viewport();
        if self.viewport.with_untracked(|v| v != &viewport) {
            self.viewport.set(viewport);
        }
        None
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        self.paint_document(cx);
    }
}
