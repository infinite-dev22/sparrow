//! Flutter DevTools shortcuts and connection status.

use std::rc::Rc;

use floem::{
    View,
    reactive::SignalGet,
    views::{Decorators, label, stack},
};

use crate::window_tab::WindowTabData;

use super::section_header;

pub(super) fn devtools_controls(window: Rc<WindowTabData>) -> impl View {
    let busy = window.devtools.busy;
    let message = window.devtools.message;
    let home = window.clone();
    let memory = window.clone();
    let network = window.clone();
    stack((
        section_header("DEVTOOLS", window.common.config),
        label(|| "Open DevTools").on_click_stop(move |_| home.open_devtools(crate::dart::devtools::DevToolsPage::Home)).disabled(move || busy.get()),
        label(|| "Profile Memory").on_click_stop(move |_| memory.open_devtools(crate::dart::devtools::DevToolsPage::Memory)).disabled(move || busy.get()),
        label(|| "Profile Network").on_click_stop(move |_| network.open_devtools(crate::dart::devtools::DevToolsPage::Network)).disabled(move || busy.get()),
        label(move || message.get()).style(|s| s.selectable(true).font_size(11.0)),
        label(|| "Run in debug or profile mode to connect. Use profile mode for representative performance measurements.").style(|s| s.font_size(11.0)),
    )).style(|s| s.flex_col().gap(8.0).padding(10.0))
}
