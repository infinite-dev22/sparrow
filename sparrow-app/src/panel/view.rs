mod dock;
mod island;
mod resize;
mod sections;
mod stripe;

pub use dock::panel_container_view;
pub use island::{ISLAND_RADIUS, island_corners};
pub use sections::PanelBuilder;
pub use stripe::tool_window_stripe;

pub use crate::widgets::foldable_panel_section;
