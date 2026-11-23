use std::{sync::Arc, time::Duration};

use floem::{
    AnyView, View,
    event::EventListener,
    reactive::{ReadSignal, SignalGet, SignalUpdate, create_rw_signal},
    style::{ScaleY, Transition},
    text::Weight,
    unit::Pct,
    views::{Decorators, label, stack, stack_from_iter},
};

use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};

/// Panel tabs and other persistent content stay visible as the action group
/// animates. Reserving space for the group keeps tab widths stable on hover.
pub fn panel_title_bar(
    title: &'static str,
    subtitle: Option<&'static str>,
    content: impl View + 'static,
    actions: Option<AnyView>,
    close: impl Fn() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let hovered = create_rw_signal(false);
    let actions = stack((
        stack_from_iter(actions),
        super::clickable_icon(
            || SparrowIcons::WINDOW_MINIMIZE,
            close,
            || false,
            || false,
            move || format!("Hide {title}"),
            config,
        ),
    ))
    .disabled(move || !hovered.get())
    .style(move |s| {
        let visible = hovered.get();
        s.items_center()
            .gap(2.0)
            .flex_shrink(0.0_f32)
            // A zero scale makes Floem's renderer transform singular.
            .scale_y(Pct(if visible { 100.0 } else { 0.01 }))
            .transition(ScaleY, Transition::ease_in_out(Duration::from_millis(140)))
            .apply_if(!visible, |s| s.pointer_events_none())
    });

    stack((
        stack((
            label(move || title).style(|s| {
                s.font_weight(Weight(600))
                    .min_width(0.0)
                    .text_ellipsis()
                    .selectable(false)
            }),
            stack_from_iter(subtitle.map(|subtitle| {
                label(move || subtitle).style(move |s| {
                    s.min_width(0.0).text_ellipsis().selectable(false).color(
                        config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                    )
                })
            }))
            .style(move |s| {
                s.min_width(0.0).apply_if(subtitle.is_none(), |s| s.hide())
            }),
            content.style(|s| s.min_width(0.0)),
        ))
        .style(|s| s.items_center().flex_grow(1.0_f32).min_width(0.0).gap(8.0)),
        actions,
    ))
    .on_event_cont(EventListener::PointerEnter, move |_| hovered.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| hovered.set(false))
    .style(move |s| {
        let config = config.get();
        s.height(config.ui.header_height().max(config.ui.icon_size() + 12) as f64)
            .font_size(13.0)
            .width_full()
            .min_width(0.0)
            .items_center()
            .padding_horiz(8.0)
            .gap(4.0)
            .flex_shrink(0.0_f32)
            .border_bottom(1.0)
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
    })
}
