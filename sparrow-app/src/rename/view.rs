//! Rename dialog with preview and apply actions.

use std::rc::Rc;

use crate::{
    config::color::SparrowColor, widgets::text_input::TextInputBuilder,
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::EventListener,
    peniko::kurbo::Point,
    reactive::{SignalGet, SignalUpdate},
    views::{Decorators, container, empty, label, stack},
};

pub(crate) fn rename(window_tab_data: Rc<WindowTabData>) -> impl View {
    let data = window_tab_data.rename.clone();
    let editor = data.editor.clone();
    let active = data.active;
    let pending = data.pending;
    let error = data.error;
    let layout_rect = data.layout_rect;
    let config = window_tab_data.common.config;
    let apply = data.clone();
    let preview = data.clone();
    let cancel = data.clone();
    stack((
        label(|| "Rename")
            .style(|s| s.font_bold().height(24.0).items_center()),
        container(
            TextInputBuilder::new()
                .is_focused(move || active.get() && !pending.get())
                .build_editor(editor)
                .style(|s| s.width_full()),
        )
        .disabled(move || pending.get())
        .style(move |s| {
            s.height(28.0)
                .width_full()
                .padding_horiz(6.0)
                .items_center()
                .font_family(config.get().editor.font_family.clone())
                .font_size(config.get().editor.font_size() as f32)
                .border(1.0)
                .border_radius(4.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .background(config.get().color(SparrowColor::EDITOR_BACKGROUND))
        }),
        label(move || {
            if pending.get() {
                "Renaming…".into()
            } else {
                error.get()
            }
        })
        .style(move |s| {
            s.width_full()
                .max_width(340.0)
                .padding_vert(4.0)
                .color(config.get().color(if error.get().is_empty() {
                    SparrowColor::PANEL_FOREGROUND_DIM
                } else {
                    SparrowColor::SPARROW_ERROR
                }))
                .apply_if(!pending.get() && error.get().is_empty(), |s| s.hide())
        }),
        stack((
            label(|| "Preview")
                .on_click_stop(move |_| preview.confirm(true))
                .style(move |s| {
                    s.padding_horiz(8.0)
                        .height(26.0)
                        .items_center()
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                        .apply_if(pending.get(), |s| {
                            s.color(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_FOREGROUND_DIM),
                            )
                        })
                }),
            empty().style(|s| s.flex_grow(1.0_f32)),
            label(|| "Cancel")
                .on_click_stop(move |_| cancel.cancel())
                .style(|s| s.padding_horiz(8.0).height(26.0).items_center()),
            label(|| "Refactor  ↵")
                .on_click_stop(move |_| apply.confirm(false))
                .style(move |s| {
                    s.padding_horiz(8.0).height(26.0).items_center().color(
                        config.get().color(if pending.get() {
                            SparrowColor::PANEL_FOREGROUND_DIM
                        } else {
                            SparrowColor::EDITOR_LINK
                        }),
                    )
                }),
        ))
        .style(|s| s.width_full().items_center()),
    ))
    .on_resize(move |rect| layout_rect.set(rect))
    .on_event_stop(EventListener::PointerMove, |_| {})
    .on_event_stop(EventListener::PointerDown, |_| {})
    .style(move |s| {
        let bounds = window_tab_data.common.workbench_size.get();
        let height=layout_rect.get().height().max(180.0);
        let origin=Point::new(((bounds.width-360.0)/2.0).max(0.0),((bounds.height-height)/2.0).max(0.0));
        s.absolute()
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .width(360.0)
            .flex_col()
            .padding(10.0)
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
            .border_radius(6.0)
            .apply_if(!active.get(), |s| s.hide())
    })
    .debug_name("Rename Layer")
}
