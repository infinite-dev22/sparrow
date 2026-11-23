//! Project tool window title and actions.

use std::rc::Rc;

use floem::{
    IntoView, View,
    reactive::{SignalGet, SignalWith},
    views::{Decorators, empty, stack},
};

use crate::{
    config::icon::SparrowIcons,
    panel::{kind::PanelKind, position::PanelPosition},
    widgets::{IconButtonStyle, icon_button, panel_title_bar},
    window_tab::WindowTabData,
};

pub(super) fn project_header(window: Rc<WindowTabData>) -> impl View {
    use crate::widgets::popup_menu::{Menu, MenuItem, PopupMenuExt};
    let config = window.common.config;
    let new_data = window.file_explorer.clone();
    let options = window.clone();
    let hide = window.panel.clone();
    let button = move |icon, tip| {
        icon_button(move || icon, move || tip, config, IconButtonStyle::default)
    };
    let actions = stack((
        button(SparrowIcons::ADD, "New").ide_popout_menu(move || {
            let base = new_data
                .select
                .get_untracked()
                .and_then(|kind| kind.path().map(ToOwned::to_owned))
                .map(|p| {
                    if p.is_dir() {
                        p
                    } else {
                        p.parent().unwrap_or(&p).to_path_buf()
                    }
                })
                .unwrap_or_else(|| new_data.root.with_untracked(|r| r.path.clone()));
            new_data.creation_menu(base)
        }),
        button(SparrowIcons::WINDOW_MORE, "More").ide_popout_menu(move || {
            let refresh = options.file_explorer.clone();
            let mut menu = Menu::new("Project Options")
                .entry(MenuItem::new("Refresh").action(move || refresh.reload()))
                .separator();
            for (title, position) in [
                ("Move to Left", PanelPosition::LeftTop),
                ("Move to Right", PanelPosition::RightTop),
                ("Move to Bottom", PanelPosition::BottomLeft),
            ] {
                let panel = options.panel.clone();
                menu = menu.entry(MenuItem::new(title).action(move || {
                    panel.move_panel_to_position(PanelKind::FileExplorer, &position)
                }));
            }
            menu
        }),
    ))
    .style(|s| s.items_center().gap(2.0));
    panel_title_bar(
        "Project",
        None,
        empty(),
        Some(actions.into_any()),
        move || hide.hide_panel(&PanelKind::FileExplorer),
        config,
    )
}
