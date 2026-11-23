//! Terminal tab strip and window actions.

use std::rc::Rc;

use floem::{
    View,
    event::{EventListener, EventPropagation},
    reactive::{SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{Decorators, container, dyn_stack, label, stack, svg},
};

use crate::panel::kind::PanelKind;
use crate::widgets::tab_strip;
use crate::{
    command::SparrowWorkbenchCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    window_tab::WindowTabData,
};

pub(super) fn terminal_tab_header(window_tab_data: Rc<WindowTabData>) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    let config = window_tab_data.common.config;
    let is_local = window_tab_data.workspace.kind.is_local();
    let tab_info = terminal.tab_info;
    let workbench_command = window_tab_data.common.workbench_command;

    let hide_panel = window_tab_data.panel.clone();
    let tabs = stack((
        tab_strip(|strip| {
            dyn_stack(
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

                    let selected =
                        move || tab_info.with(|info| info.active) == index.get();
                    let view = stack((
                        container(
                            svg(move || config.get().ui_svg(SparrowIcons::TERMINAL))
                                .style(move |s| {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;
                                    s.size(size, size).color(
                                        config.color(
                                            SparrowColor::SPARROW_ICON_ACTIVE,
                                        ),
                                    )
                                }),
                        )
                        .style(|s| s.padding_horiz(6.0)),
                        label(title).style(|s| {
                            s.min_width(0.0)
                                .max_width(180.0)
                                .text_ellipsis()
                                .selectable(false)
                        }),
                        clickable_icon(
                            || SparrowIcons::CLOSE,
                            move || terminal.close_tab(Some(terminal_tab_id)),
                            || false,
                            || false,
                            || "Close",
                            config,
                        )
                        .style(|s| s.margin_horiz(6.0).flex_shrink(0.0_f32)),
                    ))
                    .style(move |s| {
                        s.cursor(CursorStyle::Pointer)
                            .items_center()
                            .min_width(72.0)
                            .height(24.0)
                            .border_radius(4.0)
                            .border(1.0)
                            .border_color(if selected() {
                                config
                                    .get()
                                    .color(SparrowColor::SPARROW_TAB_ACTIVE_BORDER)
                            } else {
                                floem::peniko::Color::TRANSPARENT
                            })
                            .apply_if(selected(), |s| {
                                s.background(config.get().color(
                                    SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                ))
                            })
                    })
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
                    );
                    strip.item(view, selected)
                },
            )
            .style(|s| s.height_full())
        })
        .style(|s| s.height_full()),
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
        .style(|s| s.padding_horiz(4).flex_shrink(0.0_f32)),
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
