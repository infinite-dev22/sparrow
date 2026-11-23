use floem::style::{CursorStyle, Style};

use crate::config::{SparrowConfig, color::SparrowColor};

/// Shared interaction styling for rows that keep selection and navigation in
/// their owning list or tree.
pub(crate) fn selectable_row_style(
    style: Style,
    config: &SparrowConfig,
    selected: bool,
) -> Style {
    style
        .apply_if(selected, |s| {
            s.background(config.color(SparrowColor::PANEL_SELECTION_BACKGROUND))
        })
        .hover(|s| {
            s.cursor(CursorStyle::Pointer).apply_if(!selected, |s| {
                s.background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
        })
}
