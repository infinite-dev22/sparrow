//! Floating debugger controls shown while a session is paused.

use std::rc::Rc;

use floem::reactive::create_memo;
use floem::reactive::{SignalGet, SignalWith};
use floem::style::Display;
use floem::views::{Decorators, container, stack};

use crate::config::color::SparrowColor;
use crate::config::icon::SparrowIcons;
use crate::widgets::clickable_icon;

pub fn debug_floating_toolbar(
    window_tab_data: Rc<crate::window_tab::WindowTabData>,
) -> impl floem::View {
    let config = window_tab_data.common.config;
    let terminal = window_tab_data.terminal.clone();

    let is_active = create_memo(move |_| {
        terminal
            .debug
            .daps
            .with(|daps| daps.values().any(|dap| dap.stopped.get()))
    });

    let icon_btn =
        |icon: &'static str, action: Box<dyn Fn()>, tooltip: &'static str| {
            clickable_icon(
                move || icon,
                move || action(),
                || false,
                || false,
                move || tooltip,
                config,
            )
        };

    container(
        stack((
            icon_btn(
                SparrowIcons::DEBUG_CONTINUE,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_continue(id);
                        }
                    }
                }),
                "Continue",
            ),
            icon_btn(
                SparrowIcons::DEBUG_PAUSE,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_pause(id);
                        }
                    }
                }),
                "Pause",
            ),
            icon_btn(
                SparrowIcons::DEBUG_STEP_OVER,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_step_over(id);
                        }
                    }
                }),
                "Step Over",
            ),
            icon_btn(
                SparrowIcons::DEBUG_STEP_INTO,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_step_into(id);
                        }
                    }
                }),
                "Step Into",
            ),
            icon_btn(
                SparrowIcons::DEBUG_STEP_OUT,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_step_out(id);
                        }
                    }
                }),
                "Step Out",
            ),
            crate::widgets::divider::Divider::new(config)
                .variant(crate::widgets::divider::DividerVariant::Vertical)
                .into_view()
                .style(|s| s.height(16.0).align_self(None)),
            icon_btn(
                SparrowIcons::DEBUG_RESTART,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.restart_run_debug(id);
                        }
                    }
                }),
                "Restart",
            ),
            icon_btn(
                SparrowIcons::DEBUG_STOP,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.stop_run_debug(id);
                        }
                    }
                }),
                "Stop",
            ),
        ))
        .style(move |s| {
            let c = config.get();
            s.items_center()
                .padding_horiz(8.0)
                .padding_vert(4.0)
                .gap(4.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(6.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(5.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
                .pointer_events_auto() // <--- CRITICAL: Enables clicks on the pill
        }),
    )
    .style(move |s| {
        s.absolute()
            .inset_top(60.0)
            .justify_center()
            .width_full()
            .display(if is_active.get() {
                Display::Flex
            } else {
                Display::None
            })
            .pointer_events_none() // Let clicks pass through the empty space around the pill
    })
}
