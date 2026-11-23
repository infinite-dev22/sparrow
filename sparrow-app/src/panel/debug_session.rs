//! Session tabs, debugger actions and console share one selected adapter session.
use std::rc::Rc;

use floem::{
    IntoView, View,
    peniko::Color,
    reactive::{
        SignalGet, SignalUpdate, SignalWith, create_effect, create_rw_signal,
    },
    views::{Decorators, dyn_container, dyn_stack, label, stack},
};

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    command::SparrowWorkbenchCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    debug::RunDebugMode,
    widgets::clickable_icon,
    widgets::popup_menu::{Menu, MenuItem, PopupMenuExt},
    window_tab::WindowTabData,
};

mod console;

use console::console_view;

pub(super) fn panel(
    window: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    crate::ui_test_guard::observe(window.clone());
    let config = window.common.config;
    let terminal = window.terminal.clone();
    let active = terminal.debug.active_term;
    let console_selected = create_rw_signal(true);
    let follow = create_rw_signal(true);
    let steps_visible = create_rw_signal(true);
    // A breakpoint should expose the suspended thread and locals immediately.
    let pause_terminal = terminal.clone();
    create_effect(move |previous| {
        let paused = pause_terminal
            .get_active_dap(true)
            .is_some_and(|dap| dap.stopped.get());
        if paused && previous != Some(true) {
            console_selected.set(false);
        }
        paused
    });

    let sessions = terminal.clone();
    let session_window = window.clone();
    let tabs = crate::widgets::tab_strip(|strip| {
        dyn_stack(
            move || {
                sessions
                    .run_debug_process(true)
                    .into_iter()
                    .filter(|(_, p)| p.mode == RunDebugMode::Debug)
                    .collect::<Vec<_>>()
            },
            |(id, p)| (*id, p.stopped),
            move |(id, p)| {
                let terminal = session_window.terminal.clone();
                let close_terminal = terminal.clone();
                let dap_id = p.config.dap_id;
                let content = stack((
                    label(|| "•").style(move |s| {
                        s.color(if p.stopped {
                            config.get().color(SparrowColor::PANEL_FOREGROUND_DIM)
                        } else {
                            Color::from_rgb8(89, 168, 105)
                        })
                        .margin_right(5.0)
                    }),
                    label(move || p.config.name.clone())
                        .style(|s| s.max_width(220.0).text_ellipsis()),
                    clickable_icon(
                        || SparrowIcons::CLOSE,
                        move || close_terminal.close_run_debug_session(id),
                        || false,
                        || false,
                        || "Close debug session",
                        config,
                    )
                    .style(|s| s.size(18.0, 18.0).padding(2.0).margin_left(6.0)),
                ));
                strip.button(
                    content,
                    move || active.get() == Some(id),
                    move || {
                        active.set(Some(id));
                        terminal.debug.active_dap.set(Some(dap_id));
                        crate::debug_settings::refresh(&terminal);
                    },
                    config,
                )
            },
        )
        .style(|s| s.items_center().height_full())
    })
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0.0)
            .min_width(0.0)
            .height_full()
    });

    let menu_window = window.clone();
    let hide = window.panel.clone();
    let header_actions = stack((
        clickable_icon(
            || SparrowIcons::WINDOW_MORE,
            || {},
            || false,
            || false,
            || "Debug options",
            config,
        )
        .ide_popout_menu(move || {
            let close = menu_window.terminal.clone();
            let maximize = menu_window.panel.clone();
            let edit = menu_window.common.workbench_command;
            let mut menu = Menu::new("Debug")
                .entry(
                    Menu::new("Customize Toolbar").entry(
                        MenuItem::new(if steps_visible.get_untracked() {
                            "✓ Show stepping controls"
                        } else {
                            "Show stepping controls"
                        })
                        .action(move || steps_visible.update(|show| *show = !*show)),
                    ),
                )
                .entry(MenuItem::new("Modify Run Configuration…").action(
                    move || {
                        edit.send(SparrowWorkbenchCommand::EditRunConfigurations)
                    },
                ))
                .separator()
                .entry(MenuItem::new("Close All").action(move || {
                    for (id, p) in close.run_debug_process(false) {
                        if p.mode == RunDebugMode::Debug {
                            close.close_run_debug_session(id);
                        }
                    }
                }))
                .entry(
                    MenuItem::new("Maximize / Restore")
                        .action(move || maximize.toggle_maximize(&PanelKind::Debug)),
                );
            let mut docking = Menu::new("Move to");
            for (title, position) in [
                ("Left", PanelPosition::LeftTop),
                ("Right", PanelPosition::RightTop),
                ("Bottom", PanelPosition::BottomLeft),
            ] {
                let panel = menu_window.panel.clone();
                docking = docking.entry(MenuItem::new(title).action(move || {
                    panel.move_panel_to_position(PanelKind::Debug, &position)
                }));
            }
            menu = menu.entry(docking);
            menu
        }),
        clickable_icon(
            || SparrowIcons::REMOVE,
            move || hide.hide_panel(&PanelKind::Debug),
            || false,
            || false,
            || "Hide Debug",
            config,
        ),
    ));
    let header = crate::widgets::panel_title_bar("Debug", tabs, header_actions);

    let controls_window = window.clone();
    let selected_terminal = terminal.clone();
    let controls = dyn_container(
        move || {
            selected_terminal
                .run_debug_process(true)
                .into_iter()
                .find(|(id, p)| {
                    Some(*id) == active.get() && p.mode == RunDebugMode::Debug
                })
                .map(|(id, p)| (id, p.config.dap_id, p.stopped))
        },
        move |session| {
            let Some((id, dap_id, stopped)) = session else {
                return label(
                    || "Start a debug configuration to attach the debugger.",
                )
                .style(|s| s.padding_horiz(12.0))
                .into_any();
            };
            let terminal = controls_window.terminal.clone();
            let daps = terminal.debug.daps;
            let paused = move || {
                daps.with(|daps| {
                    daps.get(&dap_id).is_some_and(|dap| dap.stopped.get())
                })
            };
            let actions = [
                (SparrowIcons::DEBUG_RESTART, "Rerun", 0),
                (SparrowIcons::DEBUG_STOP, "Stop", 1),
                (SparrowIcons::DEBUG_CONTINUE, "Resume", 2),
                (SparrowIcons::DEBUG_PAUSE, "Pause", 3),
                (SparrowIcons::DEBUG_STEP_OVER, "Step Over", 4),
                (SparrowIcons::DEBUG_STEP_INTO, "Step Into", 5),
                (SparrowIcons::DEBUG_STEP_OUT, "Step Out", 6),
            ];
            let buttons = dyn_stack(
                move || actions,
                |(_, _, action)| *action,
                move |(icon, tip, action)| {
                    let terminal = terminal.clone();
                    clickable_icon(
                        move || icon,
                        move || match action {
                            0 => {
                                terminal.restart_run_debug(id);
                            }
                            1 => {
                                terminal.stop_run_debug(id);
                            }
                            2 => {
                                terminal.dap_continue(id);
                            }
                            3 => {
                                terminal.dap_pause(id);
                            }
                            4 => {
                                terminal.dap_step_over(id);
                            }
                            5 => {
                                terminal.dap_step_into(id);
                            }
                            _ => {
                                terminal.dap_step_out(id);
                            }
                        },
                        || false,
                        move || match action {
                            0 => false,
                            1 => stopped,
                            3 => stopped || paused(),
                            _ => stopped || !paused(),
                        },
                        move || tip,
                        config,
                    )
                    .style(move |s| {
                        s.flex_shrink(0.0_f32)
                            .apply_if(action >= 4 && !steps_visible.get(), |s| {
                                s.hide()
                            })
                    })
                },
            )
            .style(|s| s.items_center());
            let flutter = controls_window
                .terminal
                .run_debug_process(false)
                .iter()
                .find(|(term, _)| *term == id)
                .is_some_and(|(_, p)| {
                    crate::dart::run_config::is_flutter_target(&p.config)
                });
            let reload_window = controls_window.clone();
            let restart_window = controls_window.clone();
            let execution_terminal = controls_window.terminal.clone();
            let execution_command = controls_window.common.internal_command;
            let workbench = controls_window.common.workbench_command;
            let tools_window = controls_window.clone();
            let settings_window = controls_window.clone();
            let mute_window = controls_window.clone();
            let breakpoint_window = controls_window.clone();
            let muted = controls_window.terminal.debug.muted;
            let action_proxy = controls_window.common.proxy.clone();
            let step_thread = move || {
                daps.with_untracked(|daps| {
                    daps.get(&dap_id)
                        .and_then(|dap| dap.thread_id.get_untracked())
                })
            };
            let step_generation = move || {
                daps.with_untracked(|daps| {
                    daps.get(&dap_id)
                        .map(|dap| dap.pause_generation.get_untracked())
                        .unwrap_or_default()
                })
            };
            let step_frame = move || {
                daps.with_untracked(|daps| {
                    daps.get(&dap_id).and_then(|dap| {
                        let thread = dap.thread_id.get_untracked()?;
                        dap.stack_traces.with_untracked(|traces| {
                            traces.get(&thread).and_then(|trace| {
                                trace
                                    .frames
                                    .with_untracked(|f| f.front().map(|f| f.id))
                            })
                        })
                    })
                })
            };
            stack((
                buttons,
                clickable_icon(|| SparrowIcons::DEBUG_DISCONNECT,
                    move || crate::debug_settings::set_muted(&mute_window, !muted.get_untracked()), move || muted.get(), || false, || "Mute Breakpoints", config),
                clickable_icon(|| SparrowIcons::DEBUG_BREAKPOINT,
                    || {}, || false, || false, || "Breakpoints", config)
                    .ide_popout_menu(move || {
                        let toggle = breakpoint_window.clone();
                        Menu::new("Breakpoints")
                            .entry(MenuItem::new("Toggle Breakpoint at Cursor").action(move || toggle.toggle_breakpoint_at_cursor()))
                            .entry(MenuItem::new("View Breakpoints").action(move || console_selected.set(false)))
                    }),
                clickable_icon(
                    || SparrowIcons::FLUTTER_HOT_RELOAD,
                    move || reload_window.reload_debug_session_after_saving(dap_id, false),
                    || false,
                    move || stopped || !flutter,
                    || "Flutter Hot Reload",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::FLUTTER_HOT_RESTART,
                    move || restart_window.reload_debug_session_after_saving(dap_id, true),
                    || false,
                    move || stopped || !flutter,
                    || "Flutter Hot Restart",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::TOOLBOX,
                    move || {
                        tools_window
                            .open_devtools(crate::dart::devtools::DevToolsPage::Home)
                    },
                    || false,
                    move || stopped || !flutter,
                    || "Open Flutter DevTools",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::WINDOW_MORE,
                    || {},
                    || false,
                    || false,
                    || "Debugger options",
                    config,
                )
                .ide_popout_menu(move || {
                    let execution_terminal = execution_terminal.clone();
                    let over = action_proxy.clone();
                    let into = action_proxy.clone();
                    let smart = action_proxy.clone();
                    Menu::new("Debugger")
                        .entry(MenuItem::new("Force Step Over").enabled(paused() && !stopped).action(move || {
                            if let Some(thread_id) = step_thread() { over.dap_advanced(dap_id, sparrow_rpc::dap_types::DebugAction::ForceStep { thread_id, into:false }); }
                        }))
                        .entry(MenuItem::new("Force Step Into").enabled(paused() && !stopped).action(move || {
                            if let Some(thread_id) = step_thread() { into.dap_advanced(dap_id, sparrow_rpc::dap_types::DebugAction::ForceStep { thread_id, into:true }); }
                        }))
                        .entry(MenuItem::new("Smart Step Into…").enabled(paused() && !stopped && step_frame().is_some()).action(move || {
                            if let Some(frame_id) = step_frame() { smart.dap_advanced(dap_id, sparrow_rpc::dap_types::DebugAction::StepTargets { frame_id, generation:step_generation() }); }
                        }))
                        .entry(crate::debug_settings::menu(settings_window.clone()))
                            .entry(MenuItem::new("Show Execution Point").enabled(paused() && !stopped).action(move || {
                                if let Some((line, path)) = execution_terminal.get_dap(id, false)
                                    .and_then(|dap| dap.breakline.get_untracked()) {
                                    execution_command.send(crate::command::InternalCommand::DebugJumpToLocation {
                                        location: crate::editor::location::EditorLocation {
                                            path,
                                            position: Some(crate::editor::location::EditorPosition::Line(line)),
                                            scroll_offset: None,
                                            ignore_unconfirmed: false,
                                            same_editor_tab: false,
                                        },
                                    });
                                }
                            }))
                        .separator()
                        .entry(MenuItem::new("Modify Run Configuration…").action(
                            move || {
                                workbench.send(
                                    SparrowWorkbenchCommand::EditRunConfigurations,
                                )
                            },
                        ))
                }),
            ))
            .style(|s| s.items_center())
            .into_any()
        },
    );
    let toolbar = crate::widgets::tab_strip(|strip| {
        stack((
            controls,
            strip.button(
                label(|| "Threads & Variables"),
                move || !console_selected.get(),
                move || console_selected.set(false),
                config,
            ),
            strip.button(
                label(|| "Console"),
                move || console_selected.get(),
                move || console_selected.set(true),
                config,
            ),
        ))
        .style(|s| s.items_center().height_full())
    })
    .style(move |s| {
        s.height(30.0)
            .width_full()
            .flex_shrink(0.0_f32)
            .border_bottom(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });

    let body_window = window.clone();
    let body = dyn_container(
        move || console_selected.get(),
        move |console| {
            if console {
                console_view(body_window.clone(), follow).into_any()
            } else {
                super::debug_view::inspector(body_window.clone(), position)
                    .into_any()
            }
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0.0)
            .min_height(0.0)
            .width_full()
    });
    stack((header, toolbar, body))
        .style(|s| s.size_full().flex_col().min_width(0.0))
        .debug_name("Debug Session Panel")
}
