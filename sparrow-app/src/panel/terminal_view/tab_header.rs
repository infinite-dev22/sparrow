//! Terminal tab strip and window actions.

use std::rc::Rc;

use floem::{
    View,
    event::{EventListener, EventPropagation},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    style::CursorStyle,
    views::{
        Decorators, container, dyn_stack, empty, label,
        stack, svg,
    },
};

use crate::panel::kind::PanelKind;
use crate::{
    command::SparrowWorkbenchCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    window_tab::{Focus, WindowTabData},
};
use crate::widgets::horizontal_scroll;

pub(super) fn terminal_tab_header(window_tab_data: Rc<WindowTabData>) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let is_local = window_tab_data.workspace.kind.is_local();
    let active_index = move || terminal.tab_info.with(|info| info.active);
    let tab_info = terminal.tab_info;
    let header_width = create_rw_signal(0.0);
    let header_height = create_rw_signal(0.0);
    let icon_width = create_rw_signal(0.0);
    let workbench_command = window_tab_data.common.workbench_command;

    let hide_panel = window_tab_data.panel.clone();
    let tabs = stack((
        horizontal_scroll(dyn_stack(
            move || {
                let tabs = terminal.tab_info.with(|info| info.tabs.clone());
                for (i, (index, _)) in tabs.iter().enumerate() {
                    if index.get_untracked() != i {
                        index.set(i);
                    }
                }
                tabs
            },
            |(_, tab)| tab.terminal_tab_id,
            move |(index, tab)| {
                let terminal = terminal.clone();
                let terminal_tab_id = tab.terminal_tab_id;

                let title = {
                    let tab = tab.clone();
                    move || {
                        let Some(terminal) = tab.active_terminal(true) else {
                            return String::new();
                        };
                        if !is_local {
                            return terminal.title.get();
                        }

                        match index.get() {
                            0 => "local".to_owned(),
                            index => format!("local {}", index + 1),
                        }
                    }
                };

                stack((
                    container({
                        stack((
                            container(
                                svg(move || {
                                    config.get().ui_svg(SparrowIcons::TERMINAL)
                                })
                                .style(move |s| {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    s.size(size, size).color(
                                        config.color(
                                            SparrowColor::SPARROW_ICON_ACTIVE,
                                        ))
                                    }),
                            )
                            .style(|s| s.padding_horiz(6.0).padding_vert(3.0)),
                            label(title).style(|s| {
                                s.min_width(0.0)
                                    .max_width(180.0)
                                    .text_ellipsis()
                                    .selectable(false)
                            }),
                            clickable_icon(
                                || SparrowIcons::CLOSE,
                                move || {
                                    terminal.close_tab(Some(terminal_tab_id));
                                },
                                || false,
                                || false,
                                || "Close",
                                config,
                            )
                            .style(|s| s.margin_horiz(6.0)),
                            empty().style(move |s| {
                                s.absolute()
                                    .width_full()
                                    .height(header_height.get() - 15.0)
                                    .border_right(1.0)
                                    .border_color(
                                        config
                                            .get()
                                            .color(SparrowColor::SPARROW_BORDER),
                                    )
                                    .pointer_events_none()
                            }),
                        ))
                        .style(move |s| {
                            s.items_center()
                                .min_width(72.0)
                                .height(24.0)
                                .border_radius(4.0)
                                .apply_if(active_index() == index.get(), |s| {
                                    s.border(1.0).background(config.get().color(
                                        SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                    ))
                                })
                                .border_color(
                                    config.get().color(SparrowColor::SPARROW_BORDER),
                                )
                        })
                    })
                    .style(|s| s.items_center()),
                    container({
                        label(|| "".to_string()).style(move |s| {
                            s.size_pct(100.0, 100.0)
                                .border_bottom(if active_index() == index.get() {
                                    0.0
                                } else {
                                    0.0
                                })
                                .border_color(config.get().color(
                                    if focus.get()
                                        == Focus::Panel(PanelKind::Terminal)
                                    {
                                        SparrowColor::SPARROW_TAB_ACTIVE_UNDERLINE
                                    } else {
                                        SparrowColor::SPARROW_TAB_INACTIVE_UNDERLINE
                                    },
                                ))
                        })
                    })
                    .style(|s| {
                        s.absolute()
                            .padding_horiz(3.0)
                            .size_pct(100.0, 100.0)
                            .pointer_events_none()
                    }),
                ))
                .style(|s| s.cursor(CursorStyle::Pointer))
                .on_event_cont(
                    EventListener::PointerDown,
                    move |_| {
                        if tab_info.with_untracked(|tab| tab.active)
                            != index.get_untracked()
                        {
                            tab_info.update(|tab| {
                                tab.active = index.get_untracked();
                            });
                        }
                    },
                )
            },
        ))
        .scroll_style(|s| s.handle_thickness(3).track_thickness(3))
        .style(|s| s.max_width_pct(80.0).min_width(0.0).height_full()),
        container(clickable_icon(
            || SparrowIcons::ADD,
            move || {
                workbench_command.send(SparrowWorkbenchCommand::NewTerminalTab);
            },
            || false,
            || false,
            || "New Terminal",
            config,
        ))
        .on_resize(move |rect| {
            let width = rect.size().width;
            if icon_width.get_untracked() != width {
                icon_width.set(width);
            }
        })
        .style(|s| s.padding_horiz(4)),
    ))
    .style(|s| s.items_center().min_width(0.0));
    crate::widgets::panel_title_bar(
        "Terminal",
        tabs,
        clickable_icon(
            || SparrowIcons::WINDOW_MINIMIZE,
            move || hide_panel.hide_panel(&PanelKind::Terminal),
            || false,
            || false,
            || "Hide Terminal",
            config,
        ),
    )
    .on_resize(move |rect| {
        let size = rect.size();
        if header_width.get_untracked() != size.width {
            header_width.set(size.width);
        }
        if header_height.get_untracked() != size.height {
            header_height.set(size.height);
        }
    })
    .on_double_click(move |_| {
        window_tab_data
            .panel
            .toggle_maximize(&crate::panel::kind::PanelKind::Terminal);
        EventPropagation::Stop
    })
    .style(move |s| {
        let config = config.get();
        s.font_size(13.0)
            .border_bottom(1.0)
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
    })
}
