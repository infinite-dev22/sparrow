//! Editor hover popup.

use std::{rc::Rc, sync::atomic::AtomicU64};

use crate::widgets::sparrow_scroll;
use crate::{
    config::color::SparrowColor,
    markdown::MarkdownContent,
    window_tab::{Focus, WindowTabData},
};
use floem::{
    View,
    event::EventListener,
    reactive::{SignalGet, SignalUpdate},
    views::{
        Decorators, container, dyn_stack, empty, rich_text,
        scroll::PropagatePointerWheel,
    },
};

pub(crate) fn hover(window_tab_data: Rc<WindowTabData>) -> impl View {
    let hover_data = window_tab_data.common.hover.clone();
    let config = window_tab_data.common.config;
    let id = AtomicU64::new(0);
    let layout_rect = window_tab_data.common.hover.layout_rect;

    let actions_window = window_tab_data.clone();
    sparrow_scroll(
        floem::views::stack((
            floem::views::stack((
                floem::views::label(move || hover_data.diagnostics.get().join("\n"))
                    .style(|s| s.max_width(600.0)),
                crate::widgets::button(
                    move || {
                        hover_data
                            .suggestion
                            .get()
                            .map(|title| format!("Quick fixes · {title}"))
                            .unwrap_or_else(|| "Quick fixes (Alt+Enter)".into())
                    },
                    crate::widgets::ButtonKind::Outlined,
                    config,
                )
                .on_click_stop(move |_| {
                    if let Some(editor) =
                        actions_window.main_split.active_editor.get_untracked()
                    {
                        if editor.id() == hover_data.editor_id.get_untracked() {
                            editor.go_to_position(
                                crate::editor::location::EditorPosition::Offset(
                                    hover_data.offset.get_untracked(),
                                ),
                                None,
                                None,
                            );
                            editor.show_code_actions(false);
                        }
                    }
                }),
            ))
            .style(move |s| {
                s.flex_col()
                    .gap(5)
                    .padding(8)
                    .apply_if(hover_data.diagnostics.get().is_empty(), |s| s.hide())
            }),
            dyn_stack(
                move || hover_data.content.get(),
                move |_| id.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                move |content| match content {
                    MarkdownContent::Text(text_layout)
                    | MarkdownContent::LinkedText {
                        layout: text_layout,
                        ..
                    } => container(
                        rich_text(move || text_layout.clone())
                            .style(|s| s.max_width(600.0)),
                    )
                    .style(|s| s.max_width_full()),
                    MarkdownContent::Image { .. } => container(empty()),
                    MarkdownContent::Separator => container(
                        crate::widgets::divider::Divider::new(config)
                            .spacing(5.0)
                            .into_view()
                            .style(|s| s.width_full()),
                    ),
                },
            )
            .style(|s| s.flex_col().padding_horiz(10.0).padding_vert(5.0)),
        ))
        .style(|s| s.flex_col()),
    )
    .on_resize(move |rect| {
        layout_rect.set(rect);
    })
    .on_event_stop(EventListener::PointerMove, |_| {})
    .on_event_stop(EventListener::PointerDown, |_| {})
    .style(move |s| {
        let active = window_tab_data.common.hover.active.get();
        if !active || window_tab_data.common.focus.get() == Focus::CodeAction {
            s.hide()
        } else {
            let config = config.get();
            if let Some(origin) = window_tab_data.hover_origin() {
                s.absolute()
                    .margin_left(origin.x as f32)
                    .margin_top(origin.y as f32)
                    .max_height(300.0)
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(config.color(SparrowColor::SPARROW_BORDER))
                    .background(config.color(SparrowColor::PANEL_BACKGROUND))
                    .set(PropagatePointerWheel, false)
            } else {
                s.hide()
            }
        }
    })
    .debug_name("Hover Layer")
}
