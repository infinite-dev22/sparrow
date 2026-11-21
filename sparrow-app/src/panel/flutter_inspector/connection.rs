//! Flutter VM connection status and refresh controls.

use std::sync::Arc;

use floem::{
    IntoView, View,
    reactive::{ReadSignal, SignalGet},
    views::{Decorators, empty, label, stack},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    dart::vm_service::{FlutterInspectorData, InspectorState},
    widgets::clickable_icon,
};

pub(super) fn status_bar(
    data: &FlutterInspectorData,
    config: ReadSignal<Arc<SparrowConfig>>,
    state: ReadSignal<InspectorState>,
) -> impl View + use<> {
    let data_refresh = data.clone();
    let data_disconnect = data.clone();

    stack((
        label(move || match state.get() {
            InspectorState::Disconnected => "Not connected".to_string(),
            InspectorState::Connecting => "Connecting…".to_string(),
            InspectorState::Connected => {
                let url = data_refresh
                    .vm_url
                    .get_untracked()
                    .map(|(_, u)| shorten_url(&u))
                    .unwrap_or_default();
                format!("Live · {url}")
            }
        })
        .style(move |s| {
            let c = config.get();
            s.color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                .font_size(11.0)
                .margin_left(10.0)
                .selectable(false)
        })
        .into_any(),
        empty().style(|s| s.flex_grow(1.0f32)),
        clickable_icon(
            || SparrowIcons::IMAGE_LOADING,
            move || {
                // Reconnect / refresh from scratch
                if let Some((_, url)) = data_refresh.vm_url.get_untracked() {
                    data_refresh.client.connect(url);
                }
            },
            || false,
            || false,
            || "Refresh Widget Tree",
            config,
        ),
        clickable_icon(
            || SparrowIcons::CLOSE,
            move || data_disconnect.disconnect(),
            || false,
            move || state.get() == InspectorState::Disconnected,
            || "Disconnect",
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
    })
    .debug_name("Flutter Inspector Status")
}

fn shorten_url(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}
