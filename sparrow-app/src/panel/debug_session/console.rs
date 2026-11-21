//! Debug console, output navigation and scrolling tools.

//! Session tabs, debugger actions and console share one selected adapter session.
use std::rc::Rc;

use floem::{
    IntoView, View,
    reactive::{SignalGet, SignalTrack, SignalUpdate, SignalWith},
    views::{Decorators, dyn_container, empty, scroll, stack},
};

use crate::panel::kind::PanelKind;
use crate::{
    config::icon::SparrowIcons, widgets::clickable_icon, window_tab::WindowTabData,
};

pub(super) fn console_view(
    window: Rc<WindowTabData>,
    follow: floem::reactive::RwSignal<bool>,
) -> impl View {
    let config = window.common.config;
    let terminal = window.terminal.clone();
    let active = terminal.debug.active_term;
    let body_window = window.clone();
    dyn_container(
        move || active.get(),
        move |id| {
            let Some(id) = id else {
                return empty().into_any();
            };
            // PTY-backed adapters retain the terminal's selection, ANSI and clipboard support.
            if let Some(terminal) = body_window.terminal.get_terminal(&id) {
                return crate::terminal::view::terminal_view(
                    id,
                    terminal.raw.read_only(),
                    terminal.mode.read_only(),
                    terminal.run_debug.read_only(),
                    body_window.terminal.clone(),
                    terminal.launch_error,
                    body_window.common.internal_command,
                    body_window.workspace.clone(),
                    PanelKind::Debug,
                )
                .style(|s| s.size_full())
                .into_any();
            }
            let Some(dap) = body_window.terminal.get_dap(id, false) else {
                return empty().into_any();
            };
            let dap_id = dap.dap_id;
            let logs = body_window.terminal.debug.console_output;
            let copy = move || {
                crate::menus::copy_text(&logs.with_untracked(|logs| {
                    logs.get(&dap_id).map(|log| log.text()).unwrap_or_default()
                }))
            };
            let clear = move || {
                logs.update(|logs| {
                    logs.remove(&dap_id);
                })
            };
            let controls = stack((
                clickable_icon(
                    || SparrowIcons::FOLD_UP,
                    move || follow.set(false),
                    || false,
                    || false,
                    || "Pause automatic scrolling",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::FOLD_DOWN,
                    move || follow.set(true),
                    move || follow.get(),
                    || false,
                    || "Scroll to end",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::FILE,
                    copy,
                    || false,
                    || false,
                    || "Copy console",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::SEARCH_CLEAR,
                    clear,
                    || false,
                    || false,
                    || "Clear console",
                    config,
                ),
            ))
            .style(|s| s.flex_col().width(30.0).flex_shrink(0.0_f32));
            let row_window = body_window.clone();
            let text = floem::views::virtual_stack(
                move || logs.with(|logs| logs.get(&dap_id).map(|log|log.rows()).unwrap_or_default()).into_iter().enumerate().collect::<im::Vector<_>>(),
                |(index, line)| (*index, line.clone()),
                move |(_, line)| crate::panel::debug_console::row(line, row_window.clone()),
            ).item_size_fixed(move || config.get().terminal_font_size() as f64 * 1.6)
            .style(move |s| s.flex_col().min_width_full()
                .font_family(config.get().terminal_font_family().to_owned())
                .font_size(config.get().terminal_font_size() as f32).line_height(1.6).padding(8.0));
            let output = scroll(text)
                .scroll_to(move || {
                    if follow.get() {
                        logs.track();
                        Some(floem::kurbo::Point::new(0.0, f64::MAX))
                    } else {
                        None
                    }
                })
                .style(|s| {
                    s.flex_grow(1.0_f32)
                        .flex_basis(0.0)
                        .min_width(0.0)
                        .height_full()
                });
            stack((controls, output))
                .style(|s| s.size_full())
                .into_any()
        },
    )
    .style(|s| s.size_full())
}
