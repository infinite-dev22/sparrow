use std::sync::Arc;

use floem::{
    IntoView, View,
    peniko::Color,
    reactive::{ReadSignal, SignalGet},
    style::CursorStyle,
    views::{Decorators, button},
};

use crate::config::{SparrowConfig, color::SparrowColor};

/// A selectable tab label. The caller owns selection and switches the content.
pub fn tab_button(
    content: impl IntoView + 'static,
    selected: impl Fn() -> bool + 'static,
    select: impl FnMut() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    super::button::native_button_keyboard(button(content).action(select)).style(
        move |s| {
            let config = config.get();
            let selected = selected();
            s.height(24.0)
                .items_center()
                .padding_horiz(7.0)
                .margin_right(4.0)
                .flex_shrink(0.0_f32)
                .border_radius(4.0)
                .border(1.0)
                .border_color(if selected {
                    config.color(SparrowColor::EDITOR_LINK)
                } else {
                    Color::TRANSPARENT
                })
                .apply_if(selected, |s| {
                    s.background(
                        config.color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                    )
                })
                .focus_visible(|s| {
                    s.border_color(config.color(SparrowColor::EDITOR_CARET))
                })
                .cursor(CursorStyle::Pointer)
        },
    )
}

/// A horizontal tab strip that scrolls when the labels exceed the available width.
pub fn tab_strip(content: impl IntoView + 'static) -> impl View {
    super::horizontal_scroll(content)
        .scroll_style(|s| s.hide_bars(true))
        .style(|s| s.min_width(0.0).flex_shrink(1.0_f32))
}
