//! Application menu and workspace connection controls.

use crate::widgets::popup_menu::PopupMenuExt;
use crate::widgets::popup_menu::{Menu, MenuItem};
use floem::{
    View,
    peniko::Color,
    reactive::{Memo, ReadSignal, RwSignal, SignalGet},
    style::CursorStyle,
    views::{Decorators, container, empty, stack, svg},
};
use sparrow_rpc::proxy::ProxyStatus;
use std::{rc::Rc, sync::Arc};

use crate::{
    app::window_menu,
    command::{SparrowCommand, SparrowWorkbenchCommand},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    listener::Listener,
    widgets::{not_clickable_icon, tooltip_label},
    window_tab::WindowTabData,
    workspace::SparrowWorkspace,
};

pub(super) fn application_controls(
    window_tab_data: Rc<WindowTabData>,
    workspace: Arc<SparrowWorkspace>,
    sparrow_command: Listener<SparrowCommand>,
    workbench_command: Listener<SparrowWorkbenchCommand>,
    config: ReadSignal<Arc<SparrowConfig>>,
    proxy_status: RwSignal<Option<ProxyStatus>>,
    num_window_tabs: Memo<usize>,
) -> impl View {
    let is_local = workspace.kind.is_local();
    let is_macos = cfg!(target_os = "macos");
    stack((
        empty().style(move |s| {
            let should_hide = if is_macos {
                num_window_tabs.get() > 1
            } else {
                true
            };
            s.width(75.0).apply_if(should_hide, |s| s.hide())
        }),
        container(svg(move || config.get().ui_svg(SparrowIcons::LOGO)).style(
            move |s| {
                let config = config.get();
                s.size(16.0, 16.0)
                    .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
            },
        ))
        .style(move |s| s.margin_horiz(10.0).apply_if(is_macos, |s| s.hide())),
        not_clickable_icon(
            || SparrowIcons::MENU,
            || false,
            || false,
            || "Menu",
            config,
        )
        .ide_popout_menu(move || {
            window_menu(
                sparrow_command,
                workbench_command,
                crate::menus::MenuContext::from_window(&window_tab_data),
            )
        })
        .style(move |s| {
            s.margin_left(4.0)
                .margin_right(6.0)
                .apply_if(is_macos, |s| s.hide())
        }),
        not_clickable_icon(
            || SparrowIcons::PALETTE_MENU, || false, || false, || "Quick Actions", config,
        )
        .ide_popout_menu(move || {
            crate::menus::quick_actions_menu(&config.get().ui.quick_actions, workbench_command)
                .unwrap_or_else(|| Menu::new("Quick Actions"))
        })
        .style(move |s| {
            s.apply_if(
                !config.get().ui.quick_actions.iter().any(|id| crate::menus::quick_action(id).is_some()),
                |s| s.hide(),
            )
        }),
        tooltip_label(
            config,
            container(
                svg(move || config.get().ui_svg(SparrowIcons::REMOTE)).style(
                    move |s| {
                        let config = config.get();
                        let size = (config.ui.icon_size() as f32 + 2.0).min(30.0);
                        s.size(size, size).color(if is_local {
                            config.color(SparrowColor::SPARROW_ICON_ACTIVE)
                        } else {
                            match proxy_status.get() {
                                Some(_) => Color::WHITE,
                                None => {
                                    config.color(SparrowColor::SPARROW_ICON_ACTIVE)
                                }
                            }
                        })
                    },
                ),
            ),
            || "Connect to Remote",
        )
        .ide_popout_menu(move || {
            #[allow(unused_mut)]
            let mut menu = Menu::new("").entry(
                MenuItem::new("Connect to SSH Host").action(move || {
                    workbench_command.send(SparrowWorkbenchCommand::ConnectSshHost);
                }),
            );
            if !is_local
                && proxy_status.get().is_some_and(|p| {
                    matches!(p, ProxyStatus::Connecting | ProxyStatus::Connected)
                })
            {
                menu = menu.entry(MenuItem::new("Disconnect remote").action(
                    move || {
                        workbench_command
                            .send(SparrowWorkbenchCommand::DisconnectRemote);
                    },
                ));
            }
            #[cfg(windows)]
            {
                menu = menu.entry(MenuItem::new("Connect to WSL Host").action(
                    move || {
                        workbench_command
                            .send(SparrowWorkbenchCommand::ConnectWslHost);
                    },
                ));
            }
            menu
        })
        .style(move |s| {
            let config = config.get();
            let color = if is_local {
                Color::TRANSPARENT
            } else {
                match proxy_status.get() {
                    Some(ProxyStatus::Connected) => {
                        config.color(SparrowColor::SPARROW_REMOTE_CONNECTED)
                    }
                    Some(ProxyStatus::Connecting) => {
                        config.color(SparrowColor::SPARROW_REMOTE_CONNECTING)
                    }
                    Some(ProxyStatus::Disconnected) => {
                        config.color(SparrowColor::SPARROW_REMOTE_DISCONNECTED)
                    }
                    None => Color::TRANSPARENT,
                }
            };
            s.apply_if(is_local, |s| s.hide())
                .height_pct(100.0)
                .padding_horiz(10.0)
                .items_center()
                .background(color)
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
        }),
    ))
    .style(move |s| s.height_pct(100.0).items_center())
    .debug_name("Left Side of Top Bar")
}
