//! Session tabs, debugger actions and console share one selected adapter session.
use std::rc::Rc;

use floem::{
    IntoView, View,
    peniko::Color,
    reactive::{
        SignalGet, SignalTrack, SignalUpdate, SignalWith, create_effect,
        create_rw_signal,
    },
    views::{Decorators, dyn_container, dyn_stack, empty, label, scroll, stack},
};
use sparrow_rpc::terminal::TermId;

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    app::clickable_icon,
    command::SparrowWorkbenchCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    debug::RunDebugMode,
    popup_menu::{Menu, MenuItem, PopupMenuExt},
    terminal::panel::TerminalPanelData,
    window_tab::WindowTabData,
};

fn close_session(terminal: &TerminalPanelData, id: TermId) {
    let process = terminal
        .run_debug_process(false)
        .into_iter()
        .find(|(term, _)| *term == id);
    let Some((_, process)) = process else { return };
    let was_active = terminal.debug.active_term.get_untracked() == Some(id);
    if !process.stopped {
        terminal.stop_run_debug(id);
    }
    if let Some(dap) = terminal.debug.daps.with_untracked(|daps| daps.get(&process.config.dap_id).cloned()) {
        dap.clear_pause();
    }
    terminal.close_terminal(&id);
    terminal.debug.adapter_sessions.update(|sessions| {
        sessions.remove(&process.config.dap_id);
    });
    terminal.debug.daps.update(|daps| {
        daps.remove(&process.config.dap_id);
    });
    terminal.debug.console_output.update(|logs| {
        logs.remove(&process.config.dap_id);
    });
    if was_active {
        let next = terminal
            .run_debug_process(false)
            .into_iter()
            .find(|(_, process)| process.mode == RunDebugMode::Debug);
        terminal
            .debug
            .active_term
            .set(next.as_ref().map(|(id, _)| *id));
        terminal
            .debug
            .active_dap
            .set(next.map(|(_, p)| p.config.dap_id));
    }
    crate::debug_settings::refresh(terminal);
}

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
    let tabs = scroll(
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
                stack((
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
                        move || close_session(&close_terminal, id),
                        || false,
                        || false,
                        || "Close debug session",
                        config,
                    )
                    .style(|s| s.size(18.0, 18.0).padding(2.0).margin_left(6.0)),
                ))
                .on_click_stop(move |_| {
                    active.set(Some(id));
                    terminal.debug.active_dap.set(Some(dap_id));
                    crate::debug_settings::refresh(&terminal);
                })
                .style(move |s| {
                    s.items_center()
                        .padding_horiz(8.0)
                        .height(24.0)
                        .border_radius(4.0)
                        .border(1.0)
                        .margin_right(4.0)
                        .border_color(if active.get() == Some(id) {
                            config.get().color(SparrowColor::SPARROW_BORDER)
                        } else {
                            Color::TRANSPARENT
                        })
                        .apply_if(active.get() == Some(id), |s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_CURRENT_BACKGROUND),
                            )
                        })
                        .cursor(floem::style::CursorStyle::Pointer)
                })
            },
        )
        .style(|s| s.items_center().height_full()),
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0.0)
            .min_width(0.0)
            .height_full()
    });

    let menu_window = window.clone();
    let hide = window.panel.clone();
    let header = stack((
        label(|| "Debug")
            .style(|s| s.font_bold().margin_horiz(12.0).flex_shrink(0.0_f32)),
        tabs,
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
                            close_session(&close, id);
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
    ))
    .style(|s| {
        s.height(32.0)
            .width_full()
            .items_center()
            .flex_shrink(0.0_f32)
            .padding_right(6.0)
    });

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
            let step_thread = move || daps.with_untracked(|daps|daps.get(&dap_id).and_then(|dap|dap.thread_id.get_untracked()));
            let step_generation = move || daps.with_untracked(|daps|daps.get(&dap_id).map(|dap|dap.pause_generation.get_untracked()).unwrap_or_default());
            let step_frame = move || daps.with_untracked(|daps|daps.get(&dap_id).and_then(|dap| {
                let thread = dap.thread_id.get_untracked()?;
                dap.stack_traces.with_untracked(|traces|traces.get(&thread).and_then(|trace|trace.frames.with_untracked(|f|f.front().map(|f|f.id))))
            }));
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
    let toolbar = scroll(
        stack((
            controls,
            label(|| "Threads & Variables")
                .on_click_stop(move |_| console_selected.set(false))
                .style(move |s| {
                    tab_style(s, !console_selected.get(), &config.get())
                }),
            label(|| "Console")
                .on_click_stop(move |_| console_selected.set(true))
                .style(move |s| tab_style(s, console_selected.get(), &config.get())),
        ))
        .style(|s| s.items_center().height_full()),
    )
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

fn tab_style(
    s: floem::style::Style,
    selected: bool,
    config: &crate::config::SparrowConfig,
) -> floem::style::Style {
    s.padding_horiz(10.0)
        .padding_vert(3.0)
        .margin_horiz(4.0)
        .border(1.0)
        .border_radius(4.0)
        .flex_shrink(0.0_f32)
        .border_color(if selected {
            config.color(SparrowColor::EDITOR_LINK).multiply_alpha(0.55)
        } else {
            Color::TRANSPARENT
        })
        .apply_if(selected, |s| {
            s.background(config.color(SparrowColor::PANEL_SELECTION_BACKGROUND))
        })
        .cursor(floem::style::CursorStyle::Pointer)
}

fn console_view(
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
                move |(_, line)| super::debug_console::row(line, row_window.clone()),
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
