//! Editor pane combining the scrolling document, gutter and overlays.

use std::{path::PathBuf, rc::Rc, sync::Arc};

use floem::{
    View,
    event::{Event, EventListener},
    peniko::kurbo::Rect,
    prelude::SignalTrack,
    reactive::{
        Memo, RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_memo, create_rw_signal,
    },
    style::CursorStyle,
    views::{
        Decorators, container,
        editor::view::{LineRegion, cursor_caret},
        empty,
        scroll::{PropagatePointerWheel, scroll},
        stack,
    },
};

use super::EditorData;
use crate::{
    doc::DocContent, window_tab::WindowTabData, workspace::SparrowWorkspace,
};

use super::{
    breadcrumbs::editor_breadcrumbs,
    editor_view,
    find::find_view,
    gutter::{
        editor_gutter, editor_gutter_code_actions, editor_gutter_folding_range,
    },
    inspection::inspection_widget,
};

pub fn editor_container_view(
    window_tab_data: Rc<WindowTabData>,
    workspace: Arc<SparrowWorkspace>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    editor: RwSignal<EditorData>,
) -> impl View {
    let (editor_id, find_focus, sticky_header_height, editor_view, config, doc, ed) =
        editor.with_untracked(|editor| {
            (
                editor.id(),
                editor.find_focus,
                editor.sticky_header_height,
                editor.kind,
                editor.common.config,
                editor.doc_signal(),
                editor.editor.clone(),
            )
        });

    let main_split = window_tab_data.main_split.clone();
    let editors = main_split.editors;
    let scratch_docs = main_split.scratch_docs;
    let find_editor = main_split.find_editor;
    let replace_editor = main_split.replace_editor;
    let replace_active = main_split.common.find.replace_active;
    let replace_focus = main_split.common.find.replace_focus;
    let debug_breakline = window_tab_data.terminal.breakline;

    let viewport = ed.viewport;
    let screen_lines = ed.screen_lines;

    let markdown_mode =
        create_memo(move |_| crate::markdown_preview::mode(&editor.get()));
    let markdown_preview =
        crate::markdown_preview::preview(window_tab_data.clone(), editor);
    stack((
        editor_breadcrumbs(workspace, editor.get_untracked(), config),
        find_view(
            editor,
            find_editor,
            find_focus,
            replace_editor,
            replace_active,
            replace_focus,
            is_active,
        ),
        stack((
            stack((
                editor_gutter(window_tab_data.clone(), editor),
                container(editor_gutter_code_actions(
                    editor,
                    create_memo(|_| 20.0),
                    0.0,
                ))
                .style(|s| s.width(22.0).height_full()),
                editor_gutter_folding_range(
                    window_tab_data.clone(),
                    doc,
                    screen_lines,
                    viewport,
                ),
                editor_content(editor, debug_breakline, is_active),
                empty().style(move |s| {
                    let config = config.get();
                    s.absolute()
                        .width_pct(100.0)
                        .height(sticky_header_height.get() as f32)
                        // .box_shadow_blur(5.0)
                        // .border_bottom(1.0)
                        // .border_color(
                        //     config.get_color(SparrowColor::SPARROW_BORDER),
                        // )
                        .apply_if(
                            !config.editor.sticky_header
                                || sticky_header_height.get() == 0.0
                                || !editor_view.get().is_normal(),
                            |s| s.hide(),
                        )
                }),
                inspection_widget(window_tab_data.clone(), editor),
            ))
            .style(move |s| {
                s.height_full()
                    .min_width(0.0)
                    .flex_basis(0)
                    .flex_grow(1.0f32)
                    .apply_if(markdown_mode.get() == "preview", |s| s.hide())
            }),
            markdown_preview.style(move |s| {
                s.apply_if(markdown_mode.get() == "source", |s| s.hide())
            }),
        ))
        .style(|s| s.width_full().flex_basis(0).flex_grow(1.0_f32)),
    ))
    .on_cleanup(move || {
        let editor = editor.get_untracked();
        editor.cancel_completion();
        editor.cancel_inline_completion();
        if editors.contains_untracked(editor_id) {
            // editor still exist, so it might be moved to a different editor tab
            return;
        }
        let doc = editor.doc();
        editor.scope.dispose();

        let scratch_doc_name =
            if let DocContent::Scratch { name, .. } = doc.content.get_untracked() {
                Some(name.to_string())
            } else {
                None
            };
        if let Some(name) = scratch_doc_name {
            if !scratch_docs
                .with_untracked(|scratch_docs| scratch_docs.contains_key(&name))
            {
                doc.scope.dispose();
            }
        }
    })
    .style(|s| s.flex_col().absolute().size_pct(100.0, 100.0))
    .debug_name("Editor Container")
}

fn editor_content(
    e_data: RwSignal<EditorData>,
    debug_breakline: Memo<Option<(usize, PathBuf)>>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    let (
        cursor,
        scroll_delta,
        scroll_to,
        window_origin,
        viewport,
        sticky_header_height,
        config,
        editor,
    ) = e_data.with_untracked(|editor| {
        (
            editor.cursor().read_only(),
            editor.scroll_delta().read_only(),
            editor.scroll_to(),
            editor.window_origin(),
            editor.viewport(),
            editor.sticky_header_height,
            editor.common.config,
            editor.editor.clone(),
        )
    });

    {
        create_effect(move |_| {
            is_active(true);
            let e_data = e_data.get_untracked();
            e_data.cancel_completion();
            e_data.cancel_inline_completion();
        });
    }

    let current_scroll = create_rw_signal(Rect::ZERO);

    scroll({
        let editor_content_view =
            editor_view(e_data.get_untracked(), debug_breakline, is_active).style(
                move |s| {
                    s.absolute()
                        .margin_left(6.0)
                        .margin_top(4.0)
                        .min_size_full()
                        .cursor(CursorStyle::Text)
                },
            );

        let id = editor_content_view.id();
        editor.editor_view_id.set(Some(id));

        let editor2 = editor.clone();
        editor_content_view
            .on_event_cont(EventListener::FocusGained, move |_| {
                editor.editor_view_focused.notify();
            })
            .on_event_cont(EventListener::FocusLost, move |_| {
                editor2.editor_view_focus_lost.notify();
            })
            .on_event_cont(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer_event) = event {
                    id.request_active();
                    e_data.get_untracked().pointer_down(pointer_event);
                }
            })
            .on_event_stop(EventListener::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    e_data.get_untracked().pointer_move(pointer_event);
                }
            })
            .on_event_stop(EventListener::PointerUp, move |event| {
                if let Event::PointerUp(pointer_event) = event {
                    e_data.get_untracked().pointer_up(pointer_event);
                }
            })
            .on_event_stop(EventListener::PointerLeave, move |event| {
                if let Event::PointerLeave = event {
                    e_data.get_untracked().pointer_leave();
                }
            })
    })
    .on_move(move |point| {
        window_origin.set(point);
    })
    .on_scroll(move |rect| {
        if rect.y0 != current_scroll.get_untracked().y0 {
            // only cancel completion if scrolled vertically
            let e_data = e_data.get_untracked();
            e_data.cancel_completion();
            e_data.cancel_inline_completion();
        }
        current_scroll.set(rect);
    })
    .scroll_to(move || scroll_to.get().map(|s| s.to_point()))
    .scroll_delta(move || scroll_delta.get())
    .ensure_visible(move || {
        let e_data = e_data.get_untracked();
        let cursor = cursor.get();
        let offset = cursor.offset();
        e_data.doc_signal().track();
        e_data.kind.track();

        let LineRegion { x, width, rvline } = cursor_caret(
            &e_data.editor,
            offset,
            !cursor.is_insert(),
            cursor.affinity,
        );
        let config = config.get_untracked();
        let line_height = config.editor.line_height();
        // TODO: is there a good way to avoid the calculation of the vline here?
        let vline = e_data.editor.vline_of_rvline(rvline);
        let vline = e_data.visual_line(vline.get());
        let rect = Rect::from_origin_size(
            (x, (vline * line_height) as f64),
            (width, line_height as f64),
        )
        .inflate(10.0, 0.0);

        let viewport = viewport.get_untracked();
        let smallest_distance = (viewport.y0 - rect.y0)
            .abs()
            .min((viewport.y1 - rect.y0).abs())
            .min((viewport.y0 - rect.y1).abs())
            .min((viewport.y1 - rect.y1).abs());
        let biggest_distance = (viewport.y0 - rect.y0)
            .abs()
            .max((viewport.y1 - rect.y0).abs())
            .max((viewport.y0 - rect.y1).abs())
            .max((viewport.y1 - rect.y1).abs());
        let jump_to_middle = biggest_distance > viewport.height()
            && smallest_distance > viewport.height() / 2.0;

        if jump_to_middle {
            rect.inflate(0.0, viewport.height() / 2.0)
        } else {
            let cursor_surrounding_lines =
                e_data.editor.es.with(|s| s.cursor_surrounding_lines());
            let mut rect = rect;
            rect.y0 -= (cursor_surrounding_lines * line_height) as f64
                + sticky_header_height.get_untracked();
            rect.y1 += (cursor_surrounding_lines * line_height) as f64;
            rect
        }
    })
    .style(|s| s.size_full().set(PropagatePointerWheel, false))
    .debug_name("Editor Content")
}
