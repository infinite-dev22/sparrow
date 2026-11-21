use std::sync::Arc;

use floem::{
    View,
    peniko::Color,
    reactive::{ReadSignal, RwSignal, SignalGet},
    style::CursorStyle,
    views::Decorators,
};

use crate::config::{SparrowConfig, color::SparrowColor};

pub use super::editor_input::{TextInput, TextInputBuilder};

/// Compact, themed single-line field for forms, filters and dialogs.
///
/// All field appearance lives here. Callers only supply content and layout.
/// Floem handles editing, selection, IME and clipboard behavior.
///
/// ```ignore
/// text_input(name, config).placeholder("Project name")
/// ```
pub fn text_input(
    value: RwSignal<String>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> floem::views::TextInput {
    let input = floem::views::text_input(value);
    let id = input.id();
    input
        .on_click_stop(move |_| id.request_focus())
        .style(move |s| {
            let config = config.get();
            let foreground = config.color(SparrowColor::EDITOR_FOREGROUND);
            let accent = config.color(SparrowColor::EDITOR_CARET);
            s.width_full()
                .min_width(0.0)
                .height(28.0)
                .items_center()
                .padding_horiz(6.0)
                .padding_vert(0.0)
                .color(foreground)
                .background(Color::TRANSPARENT)
                .border(1.0)
                .border_color(config.color(SparrowColor::SPARROW_BORDER))
                .border_radius(4.0)
                .cursor(CursorStyle::Text)
                .hover(move |s| s.border_color(foreground.multiply_alpha(0.5)))
                .focus(move |s| s.border_color(accent))
        })
}
