//! Compact frame sidebar and evaluation/variables workspace.
use crate::{
    config::color::SparrowColor, panel::position::PanelPosition,
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    event::{Event, EventListener},
    reactive::{SignalGet, SignalUpdate, create_rw_signal},
    style::CursorStyle,
    views::{Decorators, dyn_container, empty, label, stack},
};
use std::rc::Rc;

mod expression_input;
mod frames;
mod variables;

use frames::frames;
use variables::variables_workspace;

pub(super) fn inspector(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window.common.config;
    let show_breakpoints = create_rw_signal(false);
    let total = create_rw_signal(800.0_f64);
    let desired = create_rw_signal(370.0_f64);
    let width = move || desired.get().min((total.get() - 120.0).max(80.0));
    let left = stack((
        frames(window.clone())
            .style(|s| s.flex_grow(1.0_f32).flex_basis(0).min_height(0.0)),
        label(move || {
            if show_breakpoints.get() {
                "‹ Threads & Variables"
            } else {
                "Breakpoints…"
            }
        })
        .on_click_stop(move |_| show_breakpoints.update(|show| *show = !*show))
        .style(move |s| {
            s.height(24.0)
                .padding_horiz(8.0)
                .items_center()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .cursor(CursorStyle::Pointer)
        }),
    ))
    .style(move |s| {
        s.flex_col()
            .width(width())
            .min_width(0.0)
            .height_full()
            .flex_shrink(0.0_f32)
    });
    let divider = empty();
    let divider_id = divider.id();
    let drag = create_rw_signal(None);
    let divider = divider
        .on_event_stop(EventListener::PointerDown, move |event| {
            if let Event::PointerDown(event) = event {
                drag.set(Some(event.pos.x));
                divider_id.request_active();
            }
        })
        .on_event_stop(EventListener::PointerMove, move |event| {
            if let Event::PointerMove(event) = event {
                if let Some(start) = drag.get_untracked() {
                    desired.set(
                        (width() + event.pos.x - start)
                            .clamp(80.0, (total.get_untracked() - 120.0).max(80.0)),
                    );
                }
            }
        })
        .on_event_stop(EventListener::PointerUp, move |_| drag.set(None))
        .style(move |s| {
            s.width(1.0)
                .height_full()
                .flex_shrink(0.0_f32)
                .background(config.get().color(SparrowColor::SPARROW_BORDER))
                .cursor(CursorStyle::ColResize)
        });
    let right_window = window.clone();
    let right = dyn_container(
        move || show_breakpoints.get(),
        move |show| {
            if show {
                super::debug_view::breakpoints_view(right_window.clone()).into_any()
            } else {
                variables_workspace(right_window.clone()).into_any()
            }
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0)
            .min_width(0.0)
            .height_full()
    });
    stack((left, divider, right))
        .style(|s| s.size_full().min_width(0.0))
        .on_resize(move |rect| total.set(rect.width()))
}
