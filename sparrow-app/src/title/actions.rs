//! Run, debug, device and application actions in the title bar.

use crate::widgets::popup_menu::PopupMenuExt;
use crate::widgets::popup_menu::{Menu, MenuItem};
use floem::{
    View,
    kurbo::Point,
    peniko::Color,
    reactive::{
        Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo,
    },
    style::CursorStyle,
    views::{Decorators, container, label, stack, svg},
};
use sparrow_core::meta;
use std::path::Path;
use std::{rc::Rc, sync::Arc};

use crate::debug::RunDebugMode;
use crate::{
    command::{SparrowWorkbenchCommand, WindowCommand},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    dart::FlutterHotReloadData,
    listener::Listener,
    update::ReleaseInfo,
    widgets::{clickable_icon, not_clickable_icon, tooltip_label},
    window_tab::WindowTabData,
};

use super::{device_selector::device_selector, window_controls_view};

pub(super) fn run_actions(
    window_command: Listener<WindowCommand>,
    workbench_command: Listener<SparrowWorkbenchCommand>,
    latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    update_in_progress: RwSignal<bool>,
    num_window_tabs: Memo<usize>,
    window_maximized: RwSignal<bool>,
    config: ReadSignal<Arc<SparrowConfig>>,
    hot_reload: FlutterHotReloadData,
    device_menu_visible: RwSignal<bool>,
    device_chip_origin: RwSignal<Point>,
    window_tab_data: Rc<WindowTabData>,
) -> impl View {
    let is_flutter_project = hot_reload.is_flutter_project();
    let latest_version = create_memo(move |_| {
        let latest_release = latest_release.get();
        let latest_version =
            latest_release.as_ref().as_ref().map(|r| r.version.clone());
        if latest_version.is_some()
            && latest_version.as_deref() != Some(meta::VERSION)
        {
            latest_version
        } else {
            None
        }
    });

    let has_update = move || latest_version.with(|v| v.is_some());

    // THE RUN CONFIG SELECTOR CHIP
    let run_config_chip = {
        container(
            stack((
                svg({
                    let window = window_tab_data.clone();
                    move || {
                        let index = window.active_run_config_idx.get();
                        let icon = window.run_configs.with(|configs| {
                            configs
                                .get(index)
                                .map(|config| {
                                    if Path::new(&config.program)
                                        .file_stem()
                                        .is_some_and(|stem| {
                                            stem.eq_ignore_ascii_case("flutter")
                                        })
                                    {
                                        SparrowIcons::FLUTTER
                                    } else {
                                        SparrowIcons::DART
                                    }
                                })
                                .unwrap_or(SparrowIcons::SETTINGS)
                        });
                        config.get().ui_svg(icon)
                    }
                })
                .style(move |s| s.size(16.0, 16.0).margin_right(6.0)),
                label({
                    let wt_data = window_tab_data.clone();
                    move || {
                        let idx = wt_data.active_run_config_idx.get();
                        wt_data.run_configs.with(|c| {
                            c.get(idx).map(|c| c.name.clone()).unwrap_or_else(|| {
                                "Add Configuration...".to_string()
                            })
                        })
                    }
                })
                .style(move |s| s.selectable(false)),
                svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW))
                    .style(move |s| {
                        let size = config.get().ui.icon_size() as f32;
                        s.size(size, size).margin_left(4.0).color(
                            config.get().color(SparrowColor::SPARROW_ICON_INACTIVE),
                        )
                    }),
            ))
            .style(|s| s.items_center()),
        )
        .ide_popout_menu({
            let wt_data = window_tab_data.clone();
            move || {
                let mut menu = Menu::new("");
                let configs = wt_data.run_configs.get_untracked();
                let active = wt_data.active_run_config_idx.get_untracked();

                // Config list
                for (i, c) in configs.iter().enumerate() {
                    let c_name = c.name.clone();
                    let label = if i == active {
                        format!("✓ {}", c_name)
                    } else {
                        format!("   {}", c_name)
                    };
                    let wt_clone = wt_data.clone();
                    menu = menu.entry(MenuItem::new(label).action(move || {
                        wt_clone.active_run_config_idx.set(i);
                    }));
                }

                if !configs.is_empty() {
                    menu = menu.separator();
                }
                let wt_clone = wt_data.clone();
                menu = menu.entry(MenuItem::new("Edit Configurations...").action(
                    move || {
                        wt_clone.run_config_modal_visible.set(true);
                    },
                ));
                menu
            }
        })
        .style(move |s| {
            s.height(28.0)
                .padding_horiz(10.0)
                .margin_horiz(6.0)
                .items_center()
                .border_radius(6.0)
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
        })
    };

    let run_state = hot_reload.state;
    let is_running =
        move || run_state.get() == crate::dart::FlutterRunState::Running;
    let debug_sessions = window_tab_data.terminal.clone();
    let debug_target = create_memo(move |_| {
        let selected = debug_sessions.debug.active_term.get();
        let sessions = debug_sessions.run_debug_process(true);
        sessions.iter().filter(|(_, p)| p.mode == RunDebugMode::Debug && !p.stopped)
            .find(|(id, _)| Some(*id) == selected)
            .or_else(|| sessions.iter().rev().find(|(_, p)| p.mode == RunDebugMode::Debug && !p.stopped))
            .map(|(id, _)| (*id, debug_sessions.get_dap(*id, true).is_some_and(|dap| dap.stopped.get())))
    });
    let debugging = create_memo(move |_| debug_target.get().is_some());
    let run_button = {
        let wt_data = window_tab_data.clone();
        tooltip_label(
            config,
            container(svg(move || config.get().ui_svg(if debugging.get() {
                if debug_target.get().is_some_and(|(_, paused)| paused) {
                    SparrowIcons::DEBUG_CONTINUE
                } else { SparrowIcons::DEBUG_PAUSE }
            } else { SparrowIcons::START })).style(
                move |s| {
                    let c = config.get();
                    let size = c.ui.icon_size() as f32;
                    s.size(size, size).color(if is_running() && !debugging.get() {
                        Color::WHITE
                    } else {
                        c.color(SparrowColor::SPARROW_RUN_ICON)
                    })
                },
            ))
            .on_click_stop(move |_| {
                if let Some((id, paused)) = debug_target.get_untracked() {
                    if paused { wt_data.terminal.dap_continue(id); }
                    else { wt_data.terminal.dap_pause(id); }
                } else { wt_data.launch_selected_configuration(RunDebugMode::Run) }
            })
            .style(move |s| {
                s.padding(4.0)
                    .margin_horiz(2.0)
                    .border_radius(4.0)
                    .apply_if(is_running() && !debugging.get(), |s| {
                        s.background(Color::from_rgb8(0x2D, 0x85, 0x50))
                    })
                    .hover(|s| {
                        s.cursor(CursorStyle::Pointer).background(
                            config
                                .get()
                                .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
            }),
            move || match debug_target.get() {
                Some((_, true)) => "Resume Debugger",
                Some((_, false)) => "Pause Debugger",
                None => "Run (Shift+F10)",
            },
        )
    };

    let debug_window = window_tab_data.clone();
    let debug_button = tooltip_label(
        config,
        container(svg(move || config.get().ui_svg(SparrowIcons::DEBUG)).style(
            move |s| {
                s.size(16.0, 16.0)
                    .color(if debugging.get() { Color::WHITE } else {
                        config.get().color(SparrowColor::SPARROW_DEBUG_ICON)
                    })
            },
        ))
        .on_click_stop(move |_| {
            if let Some((id, _)) = debug_target.get_untracked() {
                debug_window.terminal.focus_terminal_in_panel(id, crate::panel::kind::PanelKind::Debug);
                debug_window.panel.show_panel(&crate::panel::kind::PanelKind::Debug);
            } else {
                debug_window.launch_selected_configuration(RunDebugMode::Debug)
            }
        })
        .style(move |s| {
            s.padding(4.0)
                .border_radius(4.0)
                .apply_if(debugging.get(), |s| s.background(Color::from_rgb8(45, 133, 80)))
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(
                        if debugging.get() { Color::from_rgb8(45, 133, 80) }
                        else { config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND) },
                    )
                })
        }),
        move || if debugging.get() { "Show Debug Session" } else { "Debug (Shift+F9)" },
    );

    let terminal = window_tab_data.terminal.clone();
    let has_running_process = create_memo(move |_| {
        is_running()
            || terminal
                .run_debug_process(true)
                .iter()
                .any(|(_, process)| !process.stopped)
    });
    let execution_window = window_tab_data.clone();
    let execution_menu = label(|| "⋮")
        .style(move |s| {
            s.size(24.0, 28.0)
                .items_center()
                .justify_center()
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(
                        config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                    .border_radius(4.0)
                })
        })
        .ide_popout_menu(move || {
            Menu::new("")
                .entry(MenuItem::new("Run Selected Configuration").action(
                    move || {
                        workbench_command
                            .send(SparrowWorkbenchCommand::RunSelectedConfiguration)
                    },
                ))
                .entry(MenuItem::new("Debug Selected Configuration").action(
                    move || {
                        workbench_command.send(
                            SparrowWorkbenchCommand::DebugSelectedConfiguration,
                        )
                    },
                ))
                .separator()
                .entry(MenuItem::new("Run/Debug Configurations...").action({
                    let window = execution_window.clone();
                    move || window.run_config_modal_visible.set(true)
                }))
        });

    // Hot Reload button (amber flash bolt)
    let hot_reload_button = {
        let hr = hot_reload.clone();
        tooltip_label(
            config,
            container(
                svg(move || config.get().ui_svg(SparrowIcons::FLUTTER_HOT_RELOAD))
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size).color(config.color(if is_running() {
                            SparrowColor::SPARROW_HOT_RELOAD_ICON
                        } else {
                            SparrowColor::SPARROW_ICON_INACTIVE
                        }))
                    }),
            )
            .disabled(move || !is_running())
            .on_click_stop(move |_| {
                hr.common
                    .workbench_command
                    .send(SparrowWorkbenchCommand::FlutterHotReload);
            })
            .style(move |s| {
                let config = config.get();
                s.padding(4.0)
                    .margin_horiz(2.0)
                    .border_radius(6.0)
                    .hover(|s| {
                        s.cursor(CursorStyle::Pointer).background(
                            config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
            }),
            || "Hot Reload (Ctrl+F5)",
        )
    };

    // Hot Restart button (blue cyclical restart)
    let hot_restart_button = {
        let hr = hot_reload.clone();
        tooltip_label(
            config,
            container(
                svg(move || config.get().ui_svg(SparrowIcons::FLUTTER_HOT_RESTART))
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size).color(config.color(if is_running() {
                            SparrowColor::SPARROW_HOT_RESTART_ICON
                        } else {
                            SparrowColor::SPARROW_ICON_INACTIVE
                        }))
                    }),
            )
            .disabled(move || !is_running())
            .on_click_stop(move |_| {
                hr.common
                    .workbench_command
                    .send(SparrowWorkbenchCommand::FlutterHotRestart);
            })
            .style(move |s| {
                let config = config.get();
                s.padding(4.0)
                    .margin_horiz(2.0)
                    .border_radius(6.0)
                    .hover(|s| {
                        s.cursor(CursorStyle::Pointer).background(
                            config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
            }),
            || "Hot Restart (Ctrl+Shift+F5)",
        )
    };

    // Stop button (crimson red stop square)
    let stop_button = {
        tooltip_label(
            config,
            container(
                svg(move || config.get().ui_svg(SparrowIcons::FLUTTER_STOP)).style(
                    move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size).color(Color::WHITE)
                    },
                ),
            )
            .disabled(move || !has_running_process.get())
            .on_click_stop(move |_| {
                // The active Debug task/session takes priority over an older
                // Flutter Run session that may still be alive.
                workbench_command.send(SparrowWorkbenchCommand::RunAndDebugStop);
            })
            .style(move |s| {
                let config = config.get();
                s.padding(4.0)
                    .margin_horiz(2.0)
                    .border_radius(4.0)
                    .background(config.color(SparrowColor::SPARROW_STOP_ICON))
                    .hover(|s| {
                        s.cursor(CursorStyle::Pointer).background(
                            config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
            }),
            || "Stop (Ctrl+F2)",
        )
        .style(move |s| s.apply_if(!has_running_process.get(), |s| s.hide()))
    };

    // Flutter Inspector toggle button
    let inspector_button = {
        let wt_data = window_tab_data.clone();
        tooltip_label(
            config,
            container(
                svg(move || config.get().ui_svg(SparrowIcons::FLUTTER_INSPECTOR))
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size)
                            .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
                    }),
            )
            .on_click_stop(move |_| {
                wt_data.toggle_panel_visual(
                    crate::panel::kind::PanelKind::FlutterInspector,
                );
            })
            .style(move |s| {
                let config = config.get();
                s.padding(4.0)
                    .margin_horiz(2.0)
                    .border_radius(6.0)
                    .hover(|s| {
                        s.cursor(CursorStyle::Pointer).background(
                            config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
            }),
            || "Flutter Inspector",
        )
    };

    stack((
        device_selector(
            hot_reload.clone(),
            config,
            device_chip_origin,
            device_menu_visible,
        ),
        run_config_chip,
        run_button,
        debug_button,
        execution_menu,
        stop_button,
        inspector_button
            .style(move |s| s.apply_if(!is_flutter_project, |s| s.hide())),
        hot_reload_button
            .style(move |s| s.apply_if(!is_flutter_project, |s| s.hide())),
        hot_restart_button.style(move |s| {
            s.apply_if(!is_running() || !is_flutter_project, |s| s.hide())
        }),
        clickable_icon(
            || SparrowIcons::SEARCH,
            move || {
                workbench_command.send(SparrowWorkbenchCommand::SearchEverywhere);
            },
            || false,
            || false,
            || "Search Everywhere",
            config,
        ),
        stack((
            not_clickable_icon(
                || SparrowIcons::SETTINGS,
                || false,
                || false,
                || "Settings",
                config,
            )
            .ide_popout_menu(move || {
                Menu::new("")
                    .entry(MenuItem::new("Command Palette").action(move || {
                        workbench_command
                            .send(SparrowWorkbenchCommand::PaletteCommand)
                    }))
                    .separator()
                    .entry(MenuItem::new("Open Settings").action(move || {
                        workbench_command.send(SparrowWorkbenchCommand::OpenSettings)
                    }))
                    .entry(MenuItem::new("Open Keyboard Shortcuts").action(
                        move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::OpenKeyboardShortcuts)
                        },
                    ))
                    .entry(MenuItem::new("Open Theme Color Settings").action(
                        move || {
                            workbench_command.send(
                                SparrowWorkbenchCommand::OpenThemeColorSettings,
                            )
                        },
                    ))
                    .separator()
                    .entry(if let Some(v) = latest_version.get_untracked() {
                        if update_in_progress.get_untracked() {
                            MenuItem::new(format!("Update in progress ({v})"))
                                .enabled(false)
                        } else {
                            MenuItem::new(format!("Restart to update ({v})")).action(
                                move || {
                                    workbench_command.send(
                                        SparrowWorkbenchCommand::RestartToUpdate,
                                    )
                                },
                            )
                        }
                    } else {
                        MenuItem::new("No update available").enabled(false)
                    })
                    .separator()
                    .entry(MenuItem::new("About Sparrow").action(move || {
                        workbench_command.send(SparrowWorkbenchCommand::ShowAbout)
                    }))
            }),
            container(label(|| "1".to_string()).style(move |s| {
                let config = config.get();
                s.font_size(10.0)
                    .color(config.color(SparrowColor::EDITOR_BACKGROUND))
                    .border_radius(100.0)
                    .margin_left(5.0)
                    .margin_top(10.0)
                    .background(config.color(SparrowColor::EDITOR_CARET))
            }))
            .style(move |s| {
                let has_update = has_update();
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .justify_end()
                    .items_end()
                    .pointer_events_none()
                    .apply_if(!has_update, |s| s.hide())
            }),
        ))
        .style(move |s| s.margin_horiz(6.0)),
        window_controls_view(
            window_command,
            true,
            num_window_tabs,
            window_maximized,
            config,
        ),
    ))
    .style(|s| s.items_center().height_pct(100.0))
    .debug_name("Right of top bar")
}
