use std::{fmt::Display, sync::Arc};

use floem::{
    View,
    reactive::{ReadSignal, SignalGet},
    style::CursorStyle,
    views::{Decorators, button, label, stack, svg},
};

use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};

use super::popup_menu::{Menu, PopupMenuExt};

/// Opens a menu whose entries are built when the user activates the button.
pub fn menu_button<T: Display + 'static>(
    text: impl Fn() -> T + 'static,
    menu: impl Fn() -> Menu + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    button(
        stack((
            label(text)
                .style(|s| s.min_width(0.0).text_ellipsis().selectable(false)),
            svg(move || config.get().ui_svg(SparrowIcons::PALETTE_MENU)).style(
                move |s| {
                    s.size(12.0, 12.0)
                        .flex_shrink(0.0_f32)
                        .color(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE))
                },
            ),
        ))
        .style(|s| s.min_width(0.0).items_center().gap(4.0)),
    )
    .ide_popout_menu(menu)
    .style(move |s| {
        let config = config.get();
        s.min_width(0.0)
            .height(24.0)
            .padding_horiz(6.0)
            .items_center()
            .border(1.0)
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
            .border_radius(4.0)
            .cursor(CursorStyle::Pointer)
            .hover(|s| {
                s.background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
            .focus_visible(|s| {
                s.border_color(config.color(SparrowColor::EDITOR_CARET))
            })
    })
}
