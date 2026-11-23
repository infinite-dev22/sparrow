//! Window minimize, maximize and close buttons.

use floem::{
    View,
    reactive::{Memo, ReadSignal, RwSignal, SignalGet},
    views::{Decorators, stack},
};
use std::sync::Arc;

use crate::{
    command::WindowCommand,
    config::{SparrowConfig, icon::SparrowIcons},
    listener::Listener,
    widgets::clickable_icon,
};

pub fn window_controls_view(
    window_command: Listener<WindowCommand>,
    is_title: bool,
    num_window_tabs: Memo<usize>,
    window_maximized: RwSignal<bool>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        clickable_icon(
            || SparrowIcons::WINDOW_MINIMIZE,
            || {
                floem::action::minimize_window();
            },
            || false,
            || false,
            || "Minimize",
            config,
        )
        .style(|s| s.margin_right(4.0).margin_left(4.0)),
        clickable_icon(
            move || {
                if window_maximized.get() {
                    SparrowIcons::WINDOW_RESTORE
                } else {
                    SparrowIcons::WINDOW_MAXIMIZE
                }
            },
            move || {
                floem::action::set_window_maximized(
                    !window_maximized.get_untracked(),
                );
            },
            || false,
            || false,
            || "Maximize",
            config,
        )
        .style(|s| s.margin_right(4.0)),
        clickable_icon(
            || SparrowIcons::WINDOW_CLOSE,
            move || {
                window_command.send(WindowCommand::CloseWindow);
            },
            || false,
            || false,
            || "Close Window",
            config,
        )
        .style(|s| s.margin_right(6.0)),
    ))
    .style(move |s| {
        s.apply_if(
            cfg!(target_os = "macos")
                || !config.get_untracked().core.custom_titlebar
                || (is_title && num_window_tabs.get() > 1),
            |s| s.hide(),
        )
    })
}
