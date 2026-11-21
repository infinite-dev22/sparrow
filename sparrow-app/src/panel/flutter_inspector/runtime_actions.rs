//! Inspector runtime toggle buttons.

use std::sync::Arc;

use floem::{
    View,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    views::{Decorators, empty, stack},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    dart::vm_service::InspectorState,
    widgets::clickable_icon,
};

pub(super) fn toggle_bar(
    client: crate::dart::vm_service::VmServiceClient,
    debug_paint: RwSignal<bool>,
    perf_overlay: RwSignal<bool>,
    slow_animations: RwSignal<bool>,
    repaint_rainbow: RwSignal<bool>,
    state: ReadSignal<InspectorState>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        quick_toggle_icon(
            || SparrowIcons::FLUTTER_INSPECTOR,
            debug_paint,
            {
                let c = client.clone();
                move |v| c.set_debug_paint(None, v)
            },
            "Debug Paint",
            state,
            config,
        ),
        quick_toggle_icon(
            || SparrowIcons::FLUTTER_PERFORMANCE,
            perf_overlay,
            {
                let c = client.clone();
                move |v| c.set_performance_overlay(None, v)
            },
            "Performance Overlay",
            state,
            config,
        ),
        quick_toggle_icon(
            || SparrowIcons::FLUTTER_HOT_RELOAD,
            slow_animations,
            {
                let c = client.clone();
                move |v| c.set_slow_animations(None, v)
            },
            "Slow Animations (5x)",
            state,
            config,
        ),
        quick_toggle_icon(
            || SparrowIcons::SYMBOL_COLOR,
            repaint_rainbow,
            {
                let c = client.clone();
                move |v| c.set_repaint_rainbow(None, v)
            },
            "Repaint Rainbow",
            state,
            config,
        ),
        empty().style(|s| s.flex_grow(1.0_f32)),
        clickable_icon(
            || SparrowIcons::FLUTTER_HOT_RESTART,
            {
                let c = client.clone();
                move || c.reassemble(None)
            },
            || false,
            move || state.get() != InspectorState::Connected,
            || "Reassemble UI Subtree",
            config,
        )
        .style(|s| s.margin_right(4.0)),
    ))
    .style(move |s| {
        let c = config.get();
        s.height(24.0)
            .items_center()
            .padding_horiz(4.0)
            .border_bottom(1.0)
            .border_color(c.color(SparrowColor::SPARROW_BORDER))
            .background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
    })
}

fn quick_toggle_icon(
    icon: impl Fn() -> &'static str + 'static,
    flag: RwSignal<bool>,
    on_toggle: impl Fn(bool) + 'static,
    tooltip: &'static str,
    state: ReadSignal<InspectorState>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    clickable_icon(
        icon,
        move || {
            flag.update(|v| {
                *v = !*v;
                on_toggle(*v);
            });
        },
        move || flag.get(),
        move || state.get() != InspectorState::Connected,
        move || tooltip,
        config,
    )
}
