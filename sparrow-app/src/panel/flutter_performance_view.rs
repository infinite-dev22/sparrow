//! Flutter Performance panel: displays rendering time metrics (UI & Raster thread counters)
//! along with diagnostic toggle controls (Debug Paint, Performance Overlay, Slow Animations, Repaint Rainbow).

use std::rc::Rc;

use floem::{
    View,
    reactive::{create_rw_signal, SignalGet},
    views::{
        Decorators, empty, label, scroll, stack,
    },
};

use super::position::PanelPosition;
use crate::{
    widgets::{clickable_icon, toggle_row},
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::InspectorState,
    window_tab::WindowTabData,
};

mod devtools;
mod metrics;

use devtools::devtools_controls;
use metrics::perf_metrics_section;

pub fn flutter_performance_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let devtools = devtools_controls(window_tab_data.clone());
    let inspector = window_tab_data.flutter_inspector.clone();
    let config = window_tab_data.common.config;
    let state = inspector.state;

    // Local toggle states for VM service runtime flags
    let debug_paint = create_rw_signal(false);
    let perf_overlay = create_rw_signal(false);
    let slow_animations = create_rw_signal(false);
    let repaint_rainbow = create_rw_signal(false);

    let client = inspector.client.clone();

    stack((
        // Status header
        stack((
            label(move || match state.get() {
                InspectorState::Connected => "Flutter VM: Connected".to_string(),
                InspectorState::Connecting => "Flutter VM: Connecting…".to_string(),
                InspectorState::Disconnected => "Flutter VM: Disconnected".to_string(),
            })
            .style(move |s| {
                let c = config.get();
                s.color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                    .font_size(11.0)
                    .margin_left(10.0)
                    .selectable(false)
            }),
            empty().style(|s| s.flex_grow(1.0f32)),
            clickable_icon(
                || SparrowIcons::FLUTTER_HOT_RELOAD,
                {
                    let c = client.clone();
                    move || c.reassemble(None)
                },
                || false,
                move || state.get() != InspectorState::Connected,
                || "Reassemble Widget Tree",
                config,
            )
            .style(|s| s.margin_right(4.0)),
        ))
        .style(move |s| {
            let c = config.get();
            s.height(26.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
        }),

        scroll(
            stack((
                // Rendering performance indicators (UI vs Raster thread counters)
                devtools,
                perf_metrics_section(config),

                // Runtime toggle controls
                section_header("RUNTIME DIAGNOSTIC TOGGLES", config),

                toggle_row(
                    "Toggle Debug Paint",
                    Some("Draw visual layout and padding boundaries"),
                    debug_paint,
                    {
                        let c = client.clone();
                        move |val| c.set_debug_paint(None, val)
                    },
                    config,
                ),
                toggle_row(
                    "Show Performance Overlay",
                    Some("Display real-time GPU/Raster and UI frame graphs"),
                    perf_overlay,
                    {
                        let c = client.clone();
                        move |val| c.set_performance_overlay(None, val)
                    },
                    config,
                ),
                toggle_row(
                    "Slow Animations (5.0x)",
                    Some("Dilate animation time to inspect UI transitions"),
                    slow_animations,
                    {
                        let c = client.clone();
                        move |val| c.set_slow_animations(None, val)
                    },
                    config,
                ),
                toggle_row(
                    "Repaint Rainbow",
                    Some("Rotate layer border colors when repainted"),
                    repaint_rainbow,
                    {
                        let c = client.clone();
                        move |val| c.set_repaint_rainbow(None, val)
                    },
                    config,
                ),
            ))
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| s.absolute().size_full()),
    ))
    .style(|s| s.absolute().size_full().flex_col())
    .debug_name("Flutter Performance Panel")
}

fn section_header(
    title: &'static str,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::SparrowConfig>>,
) -> impl View {
    label(move || title.to_string()).style(move |s| {
        let c = config.get();
        s.font_size(10.0)
            .font_bold()
            .color(c.color(SparrowColor::EDITOR_DIM))
            .padding_left(12.0)
            .padding_vert(8.0)
            .selectable(false)
    })
}
