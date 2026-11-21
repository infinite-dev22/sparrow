//! Terminal sessions, split panes and tab context actions.

use crate::widgets::popup_menu::show_context_menu;
use crate::widgets::popup_menu::{Menu, MenuItem};
use std::rc::Rc;

use floem::{
    View, ViewId,
    event::{Event, EventListener, EventPropagation},
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_stack, tab},
};
use sparrow_rpc::terminal::TermId;

use crate::panel::kind::PanelKind;
use crate::{
    command::InternalCommand,
    config::color::SparrowColor,
    listener::Listener,
    terminal::{
        panel::TerminalPanelData, tab::TerminalTabData, view::terminal_view,
    },
    window_tab::WindowTabData,
};

fn terminal_tab_split(
    terminal_panel_data: TerminalPanelData,
    terminal_tab_data: TerminalTabData,
    tab_index: usize,
) -> impl View {
    let config = terminal_panel_data.common.config;
    let internal_command = terminal_panel_data.common.internal_command;
    let workspace = terminal_panel_data.workspace.clone();
    let active = terminal_tab_data.active;
    let terminal_tab_scope = terminal_tab_data.scope;
    dyn_stack(
        move || {
            let terminals = terminal_tab_data.terminals.get();
            for (i, (index, _)) in terminals.iter().enumerate() {
                if index.get_untracked() != i {
                    index.set(i);
                }
            }
            terminals
        },
        |(_, terminal)| terminal.term_id,
        move |(index, terminal)| {
            let terminal_panel_data = terminal_panel_data.clone();
            let terminal_scope = terminal.scope;
            container({
                let terminal_view = terminal_view(
                    terminal.term_id,
                    terminal.raw.read_only(),
                    terminal.mode.read_only(),
                    terminal.run_debug.read_only(),
                    terminal_panel_data,
                    terminal.launch_error,
                    internal_command,
                    workspace.clone(),
                    PanelKind::Terminal,
                );
                let view_id = terminal_view.id();
                let have_task = terminal.run_debug.get_untracked().is_some();
                terminal_view
                    .on_event_cont(EventListener::PointerDown, move |_| {
                        active.set(index.get_untracked());
                    })
                    .on_secondary_click_stop(move |_| {
                        if have_task {
                            tab_secondary_click(
                                internal_command,
                                view_id,
                                tab_index,
                                index.get_untracked(),
                                terminal.term_id,
                            );
                        }
                    })
                    .on_event(EventListener::PointerWheel, move |event| {
                        if let Event::PointerWheel(pointer_event) = event {
                            terminal.clone().wheel_scroll(pointer_event.delta.y);
                            EventPropagation::Stop
                        } else {
                            EventPropagation::Continue
                        }
                    })
                    .on_cleanup(move || {
                        terminal_scope.dispose();
                    })
                    .style(|s| s.size_pct(100.0, 100.0))
            })
            .style(move |s| {
                s.size_pct(100.0, 100.0).padding_horiz(10.0).apply_if(
                    index.get() > 0,
                    |s| {
                        s.border_left(1.0).border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                    },
                )
            })
        },
    )
    .on_cleanup(move || {
        terminal_tab_scope.dispose();
    })
    .style(|s| s.size_pct(100.0, 100.0))
}

pub(super) fn terminal_tab_content(window_tab_data: Rc<WindowTabData>) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    tab(
        move || terminal.tab_info.with(|info| info.active),
        move || terminal.tab_info.with(|info| info.tabs.clone()),
        |(_, tab)| tab.terminal_tab_id,
        move |(tab_index, tab)| {
            terminal_tab_split(terminal.clone(), tab, tab_index.get_untracked())
        },
    )
    .style(|s| s.size_pct(100.0, 100.0))
}

fn tab_secondary_click(
    internal_command: Listener<InternalCommand>,
    view_id: ViewId,
    tab_index: usize,
    terminal_index: usize,
    term_id: TermId,
) {
    let mut menu = Menu::new("");
    menu = menu
        .entry(MenuItem::new("Stop").action(move || {
            internal_command.send(InternalCommand::StopTerminal { term_id });
        }))
        .entry(MenuItem::new("Restart").action(move || {
            internal_command.send(InternalCommand::RestartTerminal { term_id });
        }))
        .entry(MenuItem::new("Clear All").action(move || {
            internal_command.send(InternalCommand::ClearTerminalBuffer {
                view_id,
                tab_index,
                terminal_index,
            });
        }));
    show_context_menu(menu, None);
}
