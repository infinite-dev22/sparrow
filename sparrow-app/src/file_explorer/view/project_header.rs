//! Project tool window title and actions.

use crate::widgets::tooltip::TooltipExt;
use std::rc::Rc;

use floem::{
    View,
    reactive::{SignalGet, SignalWith},
    style::CursorStyle,
    views::{Decorators, label, stack, svg},
};

use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    panel::{kind::PanelKind, position::PanelPosition},
    window_tab::WindowTabData,
};

pub(super) fn project_header(window: Rc<WindowTabData>) -> impl View {
    use crate::widgets::popup_menu::{Menu, MenuItem, PopupMenuExt};
    let config = window.common.config;
    let new_data = window.file_explorer.clone();
    let options = window.clone();
    let hide = window.panel.clone();
    let button =
        move |icon: &'static str, tip: &'static str| {
            svg(move || config.get().ui_svg(icon))
                .style(move |s| {
                    s.size(16.0, 16.0)
                        .padding(5.0)
                        .flex_shrink(0.0_f32)
                        .color(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE))
                        .cursor(CursorStyle::Pointer)
                        .border_radius(4.0)
                        .hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
                .tooltip(move || label(move || tip))
        };
    let actions = stack((
        button(SparrowIcons::ADD, "New")
            .ide_popout_menu(move || {
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
                    .unwrap_or_else(|| {
                        new_data.root.with_untracked(|r| r.path.clone())
                    });
                new_data.creation_menu(base)
            }),
        button(SparrowIcons::WINDOW_MORE, "More")
            .ide_popout_menu(move || {
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
        button(SparrowIcons::REMOVE, "Hide Project")
            .on_click_stop(move |_| hide.hide_panel(&PanelKind::FileExplorer)),
    ))
    .style(|s| s.items_center().gap(12.0));
    crate::widgets::panel_title_bar("Project", floem::views::empty(), actions)
}
