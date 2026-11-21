use floem::style::Style;

use crate::config::{SparrowConfig, color::SparrowColor};

/// The shared appearance of a floating dialog, independent of its size and contents.
pub fn dialog_surface(style: Style, config: &SparrowConfig) -> Style {
    style
        .border_radius(6.0)
        .border(1.0)
        .border_color(config.color(SparrowColor::SPARROW_BORDER))
        .box_shadow_blur(16.0)
        .box_shadow_color(config.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
        .background(config.color(SparrowColor::PANEL_BACKGROUND))
        .color(config.color(SparrowColor::EDITOR_FOREGROUND))
}
