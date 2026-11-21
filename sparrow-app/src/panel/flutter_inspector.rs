//! Live widget inspector panel — reads the running Flutter app's widget
//! tree via the Dart VM service, allows property inspection, and offers
//! quick-toggle runtime controls.

use std::rc::Rc;

use floem::{
    View,
    reactive::create_rw_signal,
    views::{
        Decorators, stack,
    },
};

use super::position::PanelPosition;
use crate::window_tab::WindowTabData;

mod connection;
mod hierarchy;
mod properties;
mod runtime_actions;

use connection::status_bar;
use hierarchy::tree_and_properties_view;
use runtime_actions::toggle_bar;

pub fn flutter_inspector_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let data = window_tab_data.flutter_inspector.clone();
    let config = window_tab_data.common.config;
    let ui_line_height = window_tab_data.common.ui_line_height;
    let state = data.state.read_only();

    // VM runtime toggle flags
    let debug_paint = create_rw_signal(false);
    let perf_overlay = create_rw_signal(false);
    let slow_animations = create_rw_signal(false);
    let repaint_rainbow = create_rw_signal(false);

    let client = data.client.clone();

    stack((
        status_bar(&data, config, state),
        // Quick toggle action bar
        toggle_bar(
            client,
            debug_paint,
            perf_overlay,
            slow_animations,
            repaint_rainbow,
            state,
            config,
        ),
        // Split view: Left/Top is Widget Hierarchy, Right/Bottom is Property Inspector
        tree_and_properties_view(&data, config, ui_line_height),
    ))
    .style(|s| s.absolute().size_full().flex_col())
    .debug_name("Flutter Inspector Panel")
}
