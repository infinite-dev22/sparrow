//! Shared tool-window title bar, used by the reference layout.
use super::{kind::PanelKind, position::PanelPosition};
use crate::widgets::popup_menu::PopupMenuExt;
use crate::widgets::popup_menu::{Menu, MenuItem};
use crate::{
    config::icon::SparrowIcons,
    widgets::{IconButtonStyle, icon_button, panel_title_bar},
    window_tab::WindowTabData,
};
use floem::{
    AnyView, IntoView, View,
    views::{Decorators, stack},
};
use std::rc::Rc;
pub fn header(
    window: Rc<WindowTabData>,
    kind: PanelKind,
    title: &'static str,
    tabs: impl View + 'static,
) -> impl View {
    header_with_actions(window, kind, title, tabs, None)
}

pub(super) fn header_with_actions(
    window: Rc<WindowTabData>,
    kind: PanelKind,
    title: &'static str,
    tabs: impl View + 'static,
    actions: Option<AnyView>,
) -> impl View {
    let config = window.common.config;
    let hide = window.panel.clone();
    let menu_panel = window.panel.clone();
    let maximize = window.panel.clone();
    let actions = stack((
        floem::views::stack_from_iter(actions),
        icon_button(
            || SparrowIcons::WINDOW_MORE,
            || "Tool Window Options",
            config,
            IconButtonStyle::default,
        )
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
                    bottom.move_panel_to_position(kind, &PanelPosition::BottomLeft)
                }))
                .separator()
                .entry(MenuItem::new("Hide").action(move || close.hide_panel(&kind)))
        }),
    ))
    .style(|s| s.items_center().gap(2.0));
    panel_title_bar(
        title,
        None,
        tabs,
        Some(actions.into_any()),
        move || hide.hide_panel(&kind),
        config,
    )
    .on_double_click_stop(move |_| maximize.toggle_maximize(&kind))
}
