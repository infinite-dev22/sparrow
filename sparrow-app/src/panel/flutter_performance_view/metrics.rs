//! Rendering metric cards and their layout.

use floem::{
    View,
    reactive::SignalGet,
    views::{Decorators, container, label, stack},
};

use crate::config::color::SparrowColor;

use super::section_header;

pub(super) fn perf_metrics_section(
    config: floem::reactive::ReadSignal<
        std::sync::Arc<crate::config::SparrowConfig>,
    >,
) -> impl View {
    stack((
        section_header("FRAME RENDERING TIMES (TARGET: 16.6ms / 60fps)", config),
        stack((
            metric_card("UI Thread", "4.2 ms", "Average build time", config),
            metric_card(
                "Raster Thread",
                "3.8 ms",
                "Average GPU render time",
                config,
            ),
        ))
        .style(|s| s.padding_horiz(12.0).gap(8.0)),
    ))
    .style(|s| s.flex_col().margin_bottom(12.0))
}

fn metric_card(
    title: &'static str,
    value: &'static str,
    desc: &'static str,
    config: floem::reactive::ReadSignal<
        std::sync::Arc<crate::config::SparrowConfig>,
    >,
) -> impl View {
    container(
        stack((
            label(move || title.to_string()).style(move |s| {
                let c = config.get();
                s.font_size(11.0)
                    .color(c.color(SparrowColor::EDITOR_DIM))
                    .selectable(false)
            }),
            label(move || value.to_string()).style(move |s| {
                let c = config.get();
                s.font_size(16.0)
                    .font_bold()
                    .color(c.color(SparrowColor::SPARROW_WARN))
                    .selectable(false)
                    .margin_vert(2.0)
            }),
            label(move || desc.to_string()).style(move |s| {
                let c = config.get();
                s.font_size(10.0)
                    .color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                    .selectable(false)
            }),
        ))
        .style(|s| s.flex_col()),
    )
    .style(move |s| {
        let c = config.get();
        s.flex_grow(1.0_f32)
            .padding(10.0)
            .border(1.0)
            .border_color(c.color(SparrowColor::SPARROW_BORDER))
            .border_radius(4.0)
            .background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
    })
}
