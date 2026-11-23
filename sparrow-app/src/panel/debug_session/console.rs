//! Debug console, output navigation and scrolling tools.

use std::rc::Rc;

use floem::{
    IntoView, View,
    reactive::{SignalGet, SignalTrack, SignalUpdate, SignalWith},
    views::{Decorators, dyn_container, empty, stack},
};

use crate::{
    config::icon::SparrowIcons,
    panel::kind::PanelKind,
    widgets::{clickable_icon, sparrow_scroll},
    window_tab::{Focus, WindowTabData},
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
        move || {
            active.get().map(|id| {
                (
                    id,
                    terminal
                        .console_terminal(id, true)
                        .map(|terminal| terminal.term_id),
                )
            })
        },
        move |session| {
            let Some((id, pty)) = session else {
                return empty().into_any();
            };
            if let Some(terminal) =
                pty.and_then(|id| body_window.terminal.get_terminal(&id))
            {
                let focus = body_window.common.focus;
                return crate::terminal::view::terminal_view(terminal, move || {
                    focus.get() == Focus::Panel(PanelKind::Debug)
                        && active.get() == Some(id)
                })
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
            let output = sparrow_scroll(text)
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
