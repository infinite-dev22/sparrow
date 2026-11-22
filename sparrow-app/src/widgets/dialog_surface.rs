use floem::style::Style;

use crate::config::{SparrowConfig, color::SparrowColor};

/// The shared appearance of a floating dialog, independent of its size and contents.
pub fn dialog_surface(style: Style, config: &SparrowConfig) -> Style {
    dialog_frame(style, config)
        .box_shadow_blur(16.0)
        .box_shadow_color(config.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
}

pub(super) fn dialog_frame(style: Style, config: &SparrowConfig) -> Style {
    style
        .border_radius(6.0)
        .border(1.0)
        .border_color(config.color(SparrowColor::SPARROW_BORDER))
        .background(config.color(SparrowColor::PANEL_BACKGROUND))
        .color(config.color(SparrowColor::EDITOR_FOREGROUND))
}
