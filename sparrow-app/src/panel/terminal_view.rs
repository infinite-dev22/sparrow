use std::rc::Rc;

use floem::{
    View,
    event::EventListener,
    reactive::{SignalGet, SignalUpdate},
    views::{
        Decorators,
        stack,
    },
};

use super::kind::PanelKind;
use crate::window_tab::{Focus, WindowTabData};

mod tab_content;
mod tab_header;

use tab_content::terminal_tab_content;
use tab_header::terminal_tab_header;

pub fn terminal_panel(window_tab_data: Rc<WindowTabData>) -> impl View {
    let focus = window_tab_data.common.focus;
    stack((
        terminal_tab_header(window_tab_data.clone()),
        terminal_tab_content(window_tab_data.clone()),
    ))
    .on_event_cont(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Panel(PanelKind::Terminal) {
            focus.set(Focus::Panel(PanelKind::Terminal));
        }
    })
    .style(|s| s.absolute().size_pct(100.0, 100.0).flex_col())
    .debug_name("Terminal Panel")
}
