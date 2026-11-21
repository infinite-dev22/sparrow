//! Editor hover popup.

use std::{rc::Rc, sync::atomic::AtomicU64};

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
        scroll::{PropagatePointerWheel, scroll},
    },
};

pub(crate) fn hover(window_tab_data: Rc<WindowTabData>) -> impl View {
    let hover_data = window_tab_data.common.hover.clone();
    let config = window_tab_data.common.config;
    let id = AtomicU64::new(0);
    let layout_rect = window_tab_data.common.hover.layout_rect;

    scroll(
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
                    crate::widgets::divider::Divider::new(config).spacing(5.0).into_view()
                        .style(|s| s.width_full()),
                ),
            },
        )
        .style(|s| s.flex_col().padding_horiz(10.0).padding_vert(5.0)),
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
