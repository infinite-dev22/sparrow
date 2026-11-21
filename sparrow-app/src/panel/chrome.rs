//! Shared tool-window title bar, used by the reference layout.
use super::{kind::PanelKind, position::PanelPosition};
use crate::widgets::popup_menu::PopupMenuExt;
use crate::widgets::popup_menu::{Menu, MenuItem};
use crate::{
    widgets::clickable_icon,
    config::{color::SparrowColor, icon::SparrowIcons},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::SignalGet,
    views::{Decorators, label, stack},
};
use std::rc::Rc;
pub fn header(
    window: Rc<WindowTabData>,
    kind: PanelKind,
    title: &'static str,
    tabs: impl View + 'static,
) -> impl View {
    let config = window.common.config;
    let hide = window.panel.clone();
    let menu_panel = window.panel.clone();
    let maximize = window.panel.clone();
    let actions = stack((
        label(|| "⋮")
            .style(|s| s.padding_horiz(8.0))
            .ide_popout_menu(move || {
                let left = menu_panel.clone();
                let right = menu_panel.clone();
                let bottom = menu_panel.clone();
                let close = menu_panel.clone();
                Menu::new("")
                    .entry(MenuItem::new("Move to Left").action(move || {
                        left.move_panel_to_position(kind, &PanelPosition::LeftTop)
                    }))
                    .entry(MenuItem::new("Move to Right").action(move || {
                        right.move_panel_to_position(kind, &PanelPosition::RightTop)
                    }))
                    .entry(MenuItem::new("Move to Bottom").action(move || {
                        bottom
                            .move_panel_to_position(kind, &PanelPosition::BottomLeft)
                    }))
                    .separator()
                    .entry(
                        MenuItem::new("Hide")
                            .action(move || close.hide_panel(&kind)),
                    )
            }),
        clickable_icon(
            || SparrowIcons::WINDOW_MINIMIZE,
            move || hide.hide_panel(&kind),
            || false,
            || false,
            || "Hide Tool Window",
            config,
        ),
    ));
    crate::widgets::panel_title_bar(title, tabs, actions)
        .on_double_click_stop(move |_| maximize.toggle_maximize(&kind))
        .style(move |s| {
            s.font_size(13.0)
                .border_bottom(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        })
}
