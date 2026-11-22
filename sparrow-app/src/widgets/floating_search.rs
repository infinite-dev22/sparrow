use std::sync::Arc;

use floem::{
    IntoView, View,
    event::EventListener,
    peniko::Color,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    style::Style,
    views::{Decorators, container, empty, stack, svg},
};

use crate::config::{SparrowConfig, color::SparrowColor};

use super::{dialog_surface::dialog_frame, modal_focus::external_dialog_active};

const CORNER_RADIUS: f64 = 10.0;

/// Gives search dialogs the same edge and elevation on every renderer.
pub(crate) fn floating_search(
    content: impl View + 'static,
    active: RwSignal<bool>,
    config: ReadSignal<Arc<SparrowConfig>>,
    top_inset: Option<f64>,
    can_dismiss: impl Fn() -> bool + Clone + 'static,
    layout: impl Fn(Style) -> Style + 'static,
) -> impl View {
    // Layer rounded fills because some renderers ignore Floem's shadow blur.
    let shadow = (1_u8..=16).rev().map(|radius| {
        let spread = f64::from(radius) * 1.5;
        empty().style(move |s| {
            s.absolute()
                .inset_left(-spread)
                .inset_right(-spread)
                .inset_top(6.0 - spread)
                .inset_bottom(-6.0 - spread)
                .border_radius(CORNER_RADIUS + spread)
                .background(
                    Color::BLACK.multiply_alpha((17.0 - f32::from(radius)) * 0.003),
                )
                .pointer_events_none()
        })
    });

    let content = content.style(move |s| {
        dialog_frame(s, &config.get())
            .width_full()
            .flex_grow(1.0_f32)
            .min_height(0)
            .border_radius(CORNER_RADIUS)
    });
    let surface = stack((
        floem::views::stack_from_iter(shadow)
            .style(|s| s.absolute().size_full().pointer_events_none()),
        content,
    ))
    .style(move |s| layout(s.flex_col().min_height(0)).max_width_pct(94.0))
    .on_event_stop(EventListener::PointerDown, |_| {});

    let dismiss = move || {
        if active.get_untracked() && !external_dialog_active() && can_dismiss() {
            active.set(false);
        }
    };
    let dismiss_outside = dismiss.clone();
    container(surface)
        .style(move |s| {
            let s = s
                .absolute()
                .size_full()
                .justify_center()
                .background(Color::BLACK.multiply_alpha(0.12));
            match top_inset {
                Some(top) => s.items_start().padding_top(top),
                None => s.items_center(),
            }
        })
        .on_event_stop(EventListener::PointerDown, move |_| dismiss_outside())
        .on_event_cont(EventListener::WindowLostFocus, move |_| dismiss())
}

/// Keeps the query and its options inside a single outlined field.
pub(crate) fn search_field(
    input: impl View + 'static,
    options: impl IntoView + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
    icon: &'static str,
) -> impl View {
    let focused = RwSignal::new(false);
    let input = input
        .on_event_cont(EventListener::FocusGained, move |_| focused.set(true))
        .on_event_cont(EventListener::FocusLost, move |_| focused.set(false))
        .style(|s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0)
                .min_width(0)
                .height(30)
                .padding_horiz(0)
                .border(0)
        });
    stack((
        svg(move || config.get().ui_svg(icon)).style(move |s| {
            s.size(15, 15)
                .flex_shrink(0.0_f32)
                .pointer_events_none()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        input,
        options,
    ))
    .style(move |s| {
        let config = config.get();
        s.width_full()
            .height(36)
            .flex_shrink(0.0_f32)
            .items_center()
            .padding_left(10)
            .padding_right(5)
            .gap(8)
            .border_radius(6)
            .background(config.color(SparrowColor::EDITOR_BACKGROUND))
            .border(1)
            .border_color(config.color(if focused.get() {
                SparrowColor::EDITOR_CARET
            } else {
                SparrowColor::SPARROW_BORDER
            }))
    })
}
