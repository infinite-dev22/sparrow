use std::sync::Arc;

use floem::{
    View,
    reactive::{
        ReadSignal, SignalGet,
    },
    views::{
        Decorators, container, label,
    },
};

use crate::config::{SparrowConfig, color::SparrowColor};

mod dock;
mod island;
mod sections;
mod stripe;

pub use dock::panel_container_view;
pub use island::{ISLAND_RADIUS, island_corners};
pub use sections::PanelBuilder;
pub use stripe::tool_window_stripe;

pub use crate::widgets::foldable_panel_section;

pub fn panel_header(
    header: String,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    container(label(move || header.clone())).style(move |s| {
        let config = config.get();
        s.padding_horiz(10.0)
            .height(config.ui.header_height() as f32)
            .items_center()
            .width_pct(100.0)
            .background(config.color(SparrowColor::PANEL_BACKGROUND))
    })
}
