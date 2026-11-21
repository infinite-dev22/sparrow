//! Terminal sessions, split panes and tab context actions.

use crate::widgets::popup_menu::show_context_menu;
use crate::widgets::popup_menu::{Menu, MenuItem};
use std::rc::Rc;

use floem::{
    View,
    event::EventListener,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_stack, tab},
};

use crate::panel::kind::PanelKind;
use crate::{
    config::color::SparrowColor,
    terminal::{
        panel::TerminalPanelData, tab::TerminalTabData, view::terminal_view,
    },
    window_tab::{Focus, WindowTabData},
};

fn terminal_tab_split(
    terminal_panel_data: TerminalPanelData,
    terminal_tab_data: TerminalTabData,
) -> impl View {
    let config = terminal_panel_data.common.config;
    let focus = terminal_panel_data.common.focus;
    let active = terminal_tab_data.active;
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
            container({
                let term_id = terminal.term_id;
                let terminal_view = terminal_view(terminal.clone(), move || {
                    focus.get() == Focus::Panel(PanelKind::Terminal)
                        && terminal_panel_data
                            .active_tab(true)
                            .and_then(|tab| tab.active_terminal(true))
                            .is_some_and(|terminal| terminal.term_id == term_id)
                });

                terminal_view
                    .on_event_cont(EventListener::PointerDown, move |_| {
                        active.set(index.get_untracked());
                    })
                    .on_secondary_click_stop(move |_| {
                        show_context_menu(
                            Menu::new("").entry(
                                MenuItem::new("Clear Buffer").action({
                                    let terminal = terminal.clone();
                                    move || terminal.clear_buffer()
                                }),
                            ),
                            None,
                        );
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
    .style(|s| s.size_pct(100.0, 100.0))
}

pub(super) fn terminal_tab_content(window_tab_data: Rc<WindowTabData>) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    tab(
        move || terminal.tab_info.with(|info| info.active),
        move || terminal.tab_info.with(|info| info.tabs.clone()),
        |(_, tab)| tab.terminal_tab_id,
        move |(_, tab)| terminal_tab_split(terminal.clone(), tab),
    )
    .style(|s| s.size_pct(100.0, 100.0))
}
