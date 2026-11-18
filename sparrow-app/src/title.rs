use crate::popup_menu::PopupMenuExt;
use crate::popup_menu::{Menu, MenuItem};
use floem::reactive::Scope;
use floem::{
    View,
    kurbo::Point,
    peniko::Color,
    prelude::create_rw_signal,
    reactive::{
        Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith,
        create_effect, create_memo, use_context,
    },
    style::{CursorStyle, Display},
    text::Weight,
    views::{
        Decorators, container, drag_window_area, dyn_stack, empty, label, scroll,
        stack, svg,
    },
};
use sparrow_core::meta;
use sparrow_rpc::proxy::ProxyStatus;
use std::path::Path;
use std::{rc::Rc, sync::Arc};

use crate::command::CommandKind;
use crate::dart::hot_reload::FlutterDevice;
use crate::debug::RunDebugMode;
use crate::text_input::TextInputBuilder;
use crate::{
    app::{clickable_icon, not_clickable_icon, tooltip_label, window_menu},
    command::{SparrowCommand, SparrowWorkbenchCommand, WindowCommand},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    dart::FlutterHotReloadData,
    db::SparrowDb,
    listener::Listener,
    update::ReleaseInfo,
    window_tab::WindowTabData,
    workspace::SparrowWorkspace,
};

fn left(
    window_tab_data: Rc<WindowTabData>,
    workspace: Arc<SparrowWorkspace>,
    sparrow_command: Listener<SparrowCommand>,
    workbench_command: Listener<SparrowWorkbenchCommand>,
    config: ReadSignal<Arc<SparrowConfig>>,
    proxy_status: RwSignal<Option<ProxyStatus>>,
    num_window_tabs: Memo<usize>,
) -> impl View {
    let is_local = workspace.kind.is_local();
    let is_macos = cfg!(target_os = "macos");
    stack((
        empty().style(move |s| {
            let should_hide = if is_macos {
                num_window_tabs.get() > 1
            } else {
                true
            };
            s.width(75.0).apply_if(should_hide, |s| s.hide())
        }),
        container(svg(move || config.get().ui_svg(SparrowIcons::LOGO)).style(
            move |s| {
                let config = config.get();
                s.size(16.0, 16.0)
                    .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
            },
        ))
        .style(move |s| s.margin_horiz(10.0).apply_if(is_macos, |s| s.hide())),
        not_clickable_icon(
            || SparrowIcons::MENU,
            || false,
            || false,
            || "Menu",
            config,
        )
        .ide_popout_menu(move || {
            window_menu(
                sparrow_command,
                workbench_command,
                crate::menus::MenuContext::from_window(&window_tab_data),
            )
        })
        .style(move |s| {
            s.margin_left(4.0)
                .margin_right(6.0)
                .apply_if(is_macos, |s| s.hide())
        }),
        tooltip_label(
            config,
            container(
                svg(move || config.get().ui_svg(SparrowIcons::REMOTE)).style(
                    move |s| {
                        let config = config.get();
                        let size = (config.ui.icon_size() as f32 + 2.0).min(30.0);
                        s.size(size, size).color(if is_local {
                            config.color(SparrowColor::SPARROW_ICON_ACTIVE)
                        } else {
                            match proxy_status.get() {
                                Some(_) => Color::WHITE,
                                None => {
                                    config.color(SparrowColor::SPARROW_ICON_ACTIVE)
                                }
                            }
                        })
                    },
                ),
            ),
            || "Connect to Remote",
        )
        .ide_popout_menu(move || {
            #[allow(unused_mut)]
            let mut menu = Menu::new("").entry(
                MenuItem::new("Connect to SSH Host").action(move || {
                    workbench_command.send(SparrowWorkbenchCommand::ConnectSshHost);
                }),
            );
            if !is_local
                && proxy_status.get().is_some_and(|p| {
                    matches!(p, ProxyStatus::Connecting | ProxyStatus::Connected)
                })
            {
                menu = menu.entry(MenuItem::new("Disconnect remote").action(
                    move || {
                        workbench_command
                            .send(SparrowWorkbenchCommand::DisconnectRemote);
                    },
                ));
            }
            #[cfg(windows)]
            {
                menu = menu.entry(MenuItem::new("Connect to WSL Host").action(
                    move || {
                        workbench_command
                            .send(SparrowWorkbenchCommand::ConnectWslHost);
                    },
                ));
            }
            menu
        })
        .style(move |s| {
            let config = config.get();
            let color = if is_local {
                Color::TRANSPARENT
            } else {
                match proxy_status.get() {
                    Some(ProxyStatus::Connected) => {
                        config.color(SparrowColor::SPARROW_REMOTE_CONNECTED)
                    }
                    Some(ProxyStatus::Connecting) => {
                        config.color(SparrowColor::SPARROW_REMOTE_CONNECTING)
                    }
                    Some(ProxyStatus::Disconnected) => {
                        config.color(SparrowColor::SPARROW_REMOTE_DISCONNECTED)
                    }
                    None => Color::TRANSPARENT,
                }
            };
            s.apply_if(is_local, |s| s.hide())
                .height_pct(100.0)
                .padding_horiz(10.0)
                .items_center()
                .background(color)
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
        }),
    ))
    .style(move |s| s.height_pct(100.0).items_center())
    .debug_name("Left Side of Top Bar")
}

fn middle(
    window_tab_data: Rc<WindowTabData>,
    _window_tabs: ReadSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    branch: impl Fn() -> String + Copy + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
    project_chip_origin: RwSignal<Point>,
    branch_chip_origin: RwSignal<Point>,
) -> impl View {
    let workspace = window_tab_data.workspace.clone();
    let _main_split = window_tab_data.main_split.clone();
    let _workbench_command = window_tab_data.common.workbench_command;
    let _window_command = window_tab_data.common.window_common.window_command;

    // Copy out everything the closures need so none of them captures the Rc.
    let _current_tab_id = window_tab_data.window_tab_id;
    let branch_menu_visible = window_tab_data.branch_menu_visible;
    let project_menu_visible = window_tab_data.project_menu_visible;

    let initial_workspace = workspace.clone();
    let project_avatar = container(
        label(move || {
            initial_workspace
                .display()
                .map(|name| project_initials(&name))
                .unwrap_or_else(|| "S".to_string())
        })
        .style(|s| s.font_size(10.0).font_bold().selectable(false)),
    )
    .style(|s| {
        s.size(18.0, 18.0)
            .items_center()
            .justify_center()
            .border_radius(5.0)
            .margin_right(7.0)
            .color(Color::WHITE)
            .background(Color::from_rgba8(0x6E, 0x65, 0xF2, 0xFF)) // AS Blue
    });

    // THE PROJECT CHIP WITH NATIVE POPOUT MENU
    let project_chip = container(
        stack((
            project_avatar,
            label(move || {
                if let Some(s) = workspace.display() {
                    s
                } else {
                    "Open Folder".to_string()
                }
            })
            .style(|s| s.selectable(false)),
            svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style(
                move |s| {
                    let size = config.get().ui.icon_size() as f32;
                    s.size(size, size).margin_left(4.0).color(
                        config.get().color(SparrowColor::SPARROW_ICON_INACTIVE),
                    )
                },
            ),
        ))
        .style(|s| s.items_center()),
    )
    .on_move(move |point| {
        if project_chip_origin.get_untracked() != point {
            project_chip_origin.set(point);
        }
    })
    .on_click_stop(move |_| {
        branch_menu_visible.set(false); // only one overlay at a time
        project_menu_visible.update(|v| *v = !*v);
    })
    .style(move |s| {
        let config = config.get();
        s.height(28.0)
            .padding_horiz(10.0)
            .items_center()
            .border_radius(6.0)
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    });

    // Branch Chip
    let branch_chip = stack((
        svg(move || config.get().ui_svg(SparrowIcons::SCM)).style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.size(size, size)
                .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
        }),
        label(branch).style(|s| s.margin_left(6.0).selectable(false)),
        svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style(
            move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                s.size(size, size)
                    .margin_left(4.0)
                    .color(config.color(SparrowColor::SPARROW_ICON_INACTIVE))
            },
        ),
    ))
    .on_move(move |point| {
        if branch_chip_origin.get_untracked() != point {
            branch_chip_origin.set(point);
        }
    })
    .on_click_stop(move |_| {
        // Toggle the branch switcher overlay
        project_menu_visible.set(false);
        branch_menu_visible.update(|v| *v = !*v);
    })
    .style(move |s| {
        let config = config.get();
        s.height(28.0)
            .padding_horiz(10.0)
            .margin_left(4.0)
            .items_center()
            .border_radius(6.0)
            .apply_if(branch().is_empty(), |s| s.hide())
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    });

    stack((
        project_chip,
        branch_chip,
        drag_window_area(empty())
            .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0f32)),
    ))
    .style(|s| {
        s.flex_basis(0)
            .flex_grow(1.0f32)
            .height_pct(100.0)
            .items_center()
    })
    .debug_name("Middle of Top Bar")
}

fn right(
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
    let run_button = {
        let wt_data = window_tab_data.clone();
        tooltip_label(
            config,
            container(svg(move || config.get().ui_svg(SparrowIcons::START)).style(
                move |s| {
                    let c = config.get();
                    let size = c.ui.icon_size() as f32;
                    s.size(size, size).color(if is_running() {
                        Color::WHITE
                    } else {
                        c.color(SparrowColor::SPARROW_RUN_ICON)
                    })
                },
            ))
            .on_click_stop(move |_| {
                wt_data.launch_selected_configuration(RunDebugMode::Run)
            })
            .style(move |s| {
                s.padding(4.0)
                    .margin_horiz(2.0)
                    .border_radius(4.0)
                    .apply_if(is_running(), |s| {
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
            || "Run (Shift+F10)",
        )
    };

    let debug_window = window_tab_data.clone();
    let debug_button = tooltip_label(
        config,
        container(svg(move || config.get().ui_svg(SparrowIcons::DEBUG)).style(
            move |s| {
                s.size(16.0, 16.0)
                    .color(config.get().color(SparrowColor::SPARROW_DEBUG_ICON))
            },
        ))
        .on_click_stop(move |_| {
            debug_window.launch_selected_configuration(RunDebugMode::Debug)
        })
        .style(move |s| {
            s.padding(4.0)
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(
                        config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
        }),
        || "Debug (Shift+F9)",
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

pub fn title(
    window_tab_data: Rc<WindowTabData>,
    window_tabs: ReadSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    project_chip_origin: RwSignal<Point>,
    branch_chip_origin: RwSignal<Point>,
    device_chip_origin: RwSignal<Point>,
) -> impl View {
    let sparrow_command = window_tab_data.common.sparrow_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let window_command = window_tab_data.common.window_common.window_command;
    let latest_release = window_tab_data.common.window_common.latest_release;
    let proxy_status = window_tab_data.common.proxy_status;
    let num_window_tabs = window_tab_data.common.window_common.num_window_tabs;
    let window_maximized = window_tab_data.common.window_common.window_maximized;
    let title_height = window_tab_data.title_height;
    let update_in_progress = window_tab_data.update_in_progress;
    let config = window_tab_data.common.config;
    let branch = window_tab_data.source_control.branch;
    let branch_text = move || branch.get();
    stack((
        left(
            window_tab_data.clone(),
            window_tab_data.workspace.clone(),
            sparrow_command,
            workbench_command,
            config,
            proxy_status,
            num_window_tabs,
        ),
        middle(
            window_tab_data.clone(),
            window_tabs,
            branch_text,
            config,
            project_chip_origin,
            branch_chip_origin,
        ),
        right(
            window_command,
            workbench_command,
            latest_release,
            update_in_progress,
            num_window_tabs,
            window_maximized,
            config,
            window_tab_data.flutter_hot_reload.clone(),
            window_tab_data.device_menu_visible,
            device_chip_origin,
            window_tab_data.clone(),
        ),
    ))
    .on_resize(move |rect| {
        let height = rect.height();
        if height != title_height.get_untracked() {
            title_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .height(config.ui.header_height().max(38) as f32)
            .flex_shrink(0.0_f32)
            .items_center()
            .background(Color::TRANSPARENT)
    })
    .debug_name("Title / Top Bar")
}

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

fn device_selector(
    hot_reload: FlutterHotReloadData,
    config: ReadSignal<Arc<SparrowConfig>>,
    origin: RwSignal<Point>,
    device_menu_visible: RwSignal<bool>,
) -> impl View {
    // Fetch devices when the app starts if it's a flutter project
    create_effect({
        let value = hot_reload.clone();
        move |_| {
            if value.is_flutter_project() {
                value.fetch_devices();
            }
        }
    });

    let value = hot_reload.clone();
    let is_flutter = move || value.is_flutter_project();

    container(
        stack((
            // Device platform icon on the chip
            svg(move || {
                let c = config.get();
                let icon_key = if let Some(dev) = hot_reload.selected_device.get() {
                    crate::dart::hot_reload::device_platform_icon(
                        dev.target_platform.as_deref(),
                    )
                } else {
                    SparrowIcons::DEVICE_DESKTOP
                };
                c.ui_svg(icon_key)
            })
            .style(move |s| {
                let c = config.get();
                let size = c.ui.icon_size() as f32 - 2.0;
                s.size(size, size)
                    .margin_right(6.0)
                    .color(Color::from_rgb8(0x5B, 0xCF, 0xF4))
            }),
            label(move || {
                if let Some(dev) = hot_reload.selected_device.get() {
                    device_display_name(&dev)
                } else {
                    "No Device".to_string()
                }
            })
            .style({
                let value = is_flutter.clone();
                move |s| s.selectable(false).apply_if(!value(), |s| s.hide())
            }),
            svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style({
                let value = is_flutter.clone();
                move |s| {
                    let c = config.get();
                    let size = c.ui.icon_size() as f32;
                    s.size(size, size)
                        .margin_left(4.0)
                        .color(c.color(SparrowColor::SPARROW_ICON_INACTIVE))
                        .apply_if(!value(), |s| s.hide())
                }
            }),
        ))
        .style(|s| s.items_center()),
    )
    .on_move(move |pt| {
        origin.set(pt);
    })
    .on_click_stop(move |_| {
        // Toggle the custom overlay
        device_menu_visible.update(|v| *v = !*v);
    })
    .style(move |s| {
        let c = config.get();
        s.height(28.0)
            .padding_horiz(10.0)
            .margin_right(8.0)
            .items_center()
            .border_radius(6.0)
            .apply_if(!is_flutter(), |s| s.hide())
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    })
    .debug_name("Flutter Device Chip")
}

fn simple_menu_item(
    text: &'static str,
    on_click: impl Fn() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    label(move || text.to_string())
        .on_click_stop(move |_| on_click())
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding_horiz(10.0)
                .padding_vert(6.0)
                .border_radius(4.0)
                .font_size(13.0)
                .hover(|s| {
                    s.background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                    .cursor(CursorStyle::Pointer)
                })
        })
}

pub fn project_switcher(
    window_tab_data: Rc<WindowTabData>,
    window_tabs: ReadSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    origin: RwSignal<Point>,
) -> impl View {
    let visible = window_tab_data.project_menu_visible;
    let config = window_tab_data.common.config;
    let window_command = window_tab_data.common.window_common.window_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let window_origin = window_tab_data.common.window_origin;
    let title_height = window_tab_data.title_height;
    let current_tab_id = window_tab_data.window_tab_id; // copied out: no Rc in closures
    let db: Arc<SparrowDb> = use_context().unwrap();

    // Recents (minus already-open projects), each with its git branch, refreshed on open.
    let recent_workspaces =
        create_rw_signal(Vec::<(Arc<SparrowWorkspace>, String)>::new());
    create_effect(move |_| {
        if visible.get() {
            if let Ok(recent) = db.recent_workspaces() {
                let open: Vec<_> = window_tabs.with_untracked(|tabs| {
                    tabs.iter().map(|(_, t)| t.workspace.path.clone()).collect()
                });
                recent_workspaces.set(
                    recent
                        .into_iter()
                        .filter(|ws| !open.contains(&ws.path))
                        .map(|ws| {
                            let branch = ws
                                .path
                                .as_deref()
                                .map(read_git_branch)
                                .unwrap_or_default();
                            (Arc::new(ws), branch)
                        })
                        .collect(),
                );
            }
        }
    });

    let close_menu = {
        let visible = window_tab_data.project_menu_visible;
        move || visible.set(false)
    };

    let section_label = move |text: &'static str| {
        label(move || text.to_string()).style(move |s| {
            s.padding_horiz(8.0)
                .padding_top(6.0)
                .padding_bottom(4.0)
                .font_size(11.0)
                .color(config.get().color(SparrowColor::EDITOR_DIM))
        })
    };

    let divider = move || {
        empty().style(move |s| {
            s.height(1.0)
                .margin_vert(6.0)
                .background(config.get().color(SparrowColor::SPARROW_BORDER))
        })
    };

    let build_project_item =
        move |workspace: Arc<SparrowWorkspace>, branch: String, is_current: bool| {
            let path_str = workspace
                .path
                .as_deref()
                .map(display_path)
                .unwrap_or_default();
            let name = workspace
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".into());
            let initials: String =
                name.chars().take(2).collect::<String>().to_uppercase();

            let hash = name.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32));
            let color = match hash % 5 {
                0 => Color::from_rgb8(0xE0, 0x7A, 0x5F),
                1 => Color::from_rgb8(0x2A, 0xA1, 0xB8),
                2 => Color::from_rgb8(0xB0, 0x4F, 0xC0),
                3 => Color::from_rgb8(0x50, 0xA1, 0x4F),
                _ => Color::from_rgb8(0x7A, 0x4F, 0xE0),
            };

            let has_branch = !branch.is_empty();
            let cmd = window_command;
            let close = close_menu;
            let ws = workspace.clone();
            let config_get = config;

            stack((
                container(label(move || initials.clone()).style(|s| {
                    s.font_size(13.0)
                        .font_weight(Weight::BOLD)
                        .color(Color::WHITE)
                        .selectable(false)
                }))
                .style(move |s| {
                    s.size(32.0, 32.0)
                        .flex_shrink(0.0_f32)
                        .border_radius(8.0)
                        .items_center()
                        .justify_center()
                        .background(color)
                        .margin_right(12.0)
                }),
                stack((
                    label(move || name.clone()).style(move |s| {
                        s.font_size(13.0).color(
                            config_get.get().color(SparrowColor::EDITOR_FOREGROUND),
                        )
                    }),
                    label(move || path_str.clone()).style(move |s| {
                        s.font_size(11.0)
                            .text_ellipsis()
                            .color(config_get.get().color(SparrowColor::EDITOR_DIM))
                    }),
                    stack((
                        svg(move || config_get.get().ui_svg(SparrowIcons::SCM))
                            .style(move |s| {
                                let c = config_get.get();
                                let size = c.ui.icon_size() as f32 - 4.0;
                                s.size(size, size)
                                    .margin_right(4.0)
                                    .color(c.color(SparrowColor::EDITOR_DIM))
                            }),
                        label(move || branch.clone()).style(move |s| {
                            s.font_size(11.0).color(
                                config_get.get().color(SparrowColor::EDITOR_DIM),
                            )
                        }),
                    ))
                    .style(move |s| {
                        s.items_center().apply_if(!has_branch, |s| s.hide())
                    }),
                ))
                .style(|s| s.flex_col().flex_grow(1.0f32).min_width(0.0)),
            ))
            .on_click_stop(move |_| {
                if !is_current {
                    cmd.send(WindowCommand::SetWorkspace {
                        workspace: (*ws).clone(),
                    });
                }
                close();
            })
            .style(move |s| {
                let c = config_get.get();
                s.width_full()
                    .padding_horiz(8.0)
                    .padding_vert(6.0)
                    .border_radius(6.0)
                    .items_center()
                    .hover(|s| {
                        s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                            .cursor(CursorStyle::Pointer)
                    })
            })
        };

    stack((
        // Click-outside catcher
        empty()
            .on_click_stop(move |_| close_menu())
            .style(|s| s.absolute().size_full()),
        // Panel
        container(
            scroll(
                stack((
                    menu_row(
                        None,
                        "New Project...",
                        move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::OpenFolder);
                            close_menu();
                        },
                        config,
                    ),
                    menu_row(
                        Some(SparrowIcons::DIRECTORY),
                        "Open...",
                        move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::OpenFile);
                            close_menu();
                        },
                        config,
                    ),
                    menu_row(
                        Some(SparrowIcons::SCM),
                        "Clone Repository...",
                        || {},
                        config,
                    ),
                    menu_row(None, "Import Sample...", || {}, config),
                    divider(),
                    section_label("Open Projects"),
                    dyn_stack(
                        move || window_tabs.get(),
                        |(_, tab)| tab.window_tab_id,
                        move |(_, tab)| {
                            build_project_item(
                                tab.workspace.clone(),
                                tab.source_control.branch.get_untracked(),
                                tab.window_tab_id == current_tab_id,
                            )
                        },
                    ),
                    divider(),
                    section_label("Recent Projects"),
                    dyn_stack(
                        move || recent_workspaces.get(),
                        |(ws, _)| ws.path.clone(),
                        move |(ws, branch)| build_project_item(ws, branch, false),
                    ),
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.max_height(520.0).width_full()),
        )
        .on_click_stop(|_| {}) // clicks inside don't close it
        .style(move |s| {
            let c = config.get();
            let pt = origin.get() - window_origin.get().to_vec2();
            s.absolute()
                .margin_left(pt.x as f32)
                .margin_top(title_height.get() as f32 + 2.0)
                .width(320.0)
                .padding(8.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(8.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(12.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .display(if visible.get() {
                Display::Flex
            } else {
                Display::None
            })
            .z_index(100)
    })
}

pub fn branch_switcher(
    window_tab_data: Rc<WindowTabData>,
    origin: RwSignal<Point>,
) -> impl View {
    let visible = window_tab_data.branch_menu_visible;
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let current_branch = source_control.branch.read_only();
    let branches = source_control.branches.read_only();
    let tags = source_control.tags.read_only();
    let sparrow_command = window_tab_data.common.sparrow_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let title_height = window_tab_data.title_height;
    let window_origin = window_tab_data.common.window_origin;

    // Setup a local editor for the search input
    let cx = Scope::current();
    let search_editor = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());
    let search_buffer = search_editor.doc_signal();

    // Clear search when opened
    create_effect({
        let value = search_editor.clone();
        move |_| {
            if visible.get() {
                value.doc().reload(lapce_xi_rope::Rope::from(""), true);
            }
        }
    });

    let close_menu = {
        let visible = window_tab_data.branch_menu_visible;
        move || visible.set(false)
    };

    let search_text = create_memo(move |_| {
        search_buffer.with(|d| d.buffer.with(|b| b.to_string().to_lowercase()))
    });

    // Section header label
    let section_label = move |text: &'static str| {
        label(move || text.to_string()).style(move |s| {
            s.padding_horiz(10.0)
                .padding_vert(4.0)
                .margin_top(4.0) // Give sections breathing room
                .font_size(11.0)
                .font_weight(Weight::BOLD)
                .color(config.get().color(SparrowColor::EDITOR_DIM))
        })
    };

    // Thin divider
    let divider = move |margin: f32| {
        empty().style(move |s| {
            s.height(1.0)
                .margin_vert(margin)
                .background(config.get().color(SparrowColor::SPARROW_BORDER))
        })
    };

    // Helper to draw a single branch/tag row
    let branch_row = move |name: String, is_current: bool| {
        let name_clone = name.clone();
        let cmd = sparrow_command;
        let close = close_menu;
        let config_get = config;

        container(
            stack((
                svg(move || config_get.get().ui_svg(SparrowIcons::SCM)).style(
                    move |s| {
                        let c = config_get.get();
                        let size = c.ui.icon_size() as f32 - 2.0;
                        s.size(size, size).margin_right(8.0).color(if is_current {
                            c.color(SparrowColor::EDITOR_FOREGROUND)
                        } else {
                            c.color(SparrowColor::EDITOR_DIM)
                        })
                    },
                ),
                label(move || name_clone.clone()).style(move |s| {
                    let c = config_get.get();
                    s.font_size(13.0)
                        .font_weight(if is_current {
                            Weight::BOLD
                        } else {
                            Weight::NORMAL
                        })
                        .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                }),
            ))
            .style(|s| s.items_center()),
        )
        .on_click_stop(move |_| {
            if !is_current {
                cmd.send(SparrowCommand {
                    kind: CommandKind::Workbench(
                        SparrowWorkbenchCommand::CheckoutReference,
                    ),
                    data: Some(serde_json::json!(name)),
                });
            }
            close();
        })
        .style(move |s| {
            let c = config_get.get();
            s.width_full()
                .height(32.0)
                .padding_horiz(10.0)
                .items_center()
                .apply_if(is_current, |s| {
                    s.background(
                        c.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND)
                            .multiply_alpha(0.3),
                    )
                })
                .hover(|s| {
                    s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                        .cursor(CursorStyle::Pointer)
                })
        })
    };

    stack((
        // Background to catch outside clicks
        empty()
            .on_click_stop(move |_| close_menu())
            .style(|s| s.absolute().size_full()),
        // Menu Panel
        container(
            scroll(
                stack((
                    // Search bar
                    container(
                        TextInputBuilder::new()
                            .build_editor(search_editor)
                            .placeholder(|| {
                                "Search for branches and actions".to_string()
                            })
                            .style(move |s| {
                                let c = config.get();
                                s.width_full()
                                    .min_height(32.0)
                                    .padding_horiz(8.0) // <-- Fixes text clipping
                                    .border(1.0)
                                    .border_radius(4.0)
                                    .border_color(
                                        c.color(SparrowColor::SPARROW_BORDER),
                                    )
                                    .background(
                                        c.color(SparrowColor::EDITOR_BACKGROUND),
                                    )
                            }),
                    )
                    .style(|s| s.padding(10.0).width_full()),
                    divider(0.0),
                    // Actions
                    stack((
                        simple_menu_item(
                            "Update Project...",
                            move || {
                                sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(SparrowWorkbenchCommand::GitCommand),data:Some(serde_json::json!(["pull","--ff-only"]))});
                                close_menu();
                            },
                            config,
                        ),
                        simple_menu_item(
                            "Commit...",
                            move || {
                                workbench_command.send(
                                    SparrowWorkbenchCommand::SourceControlCommit,
                                );
                                close_menu();
                            },
                            config,
                        ),
                        simple_menu_item("Push...", move || {sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(SparrowWorkbenchCommand::GitCommand),data:Some(serde_json::json!(["push"]))});}, config),
                    ))
                    .style(|s| s.flex_col().width_full()),
                    divider(6.0),
                    // Branch Actions
                    simple_menu_item("New Branch...", move || {sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(SparrowWorkbenchCommand::GitCommand),data:Some(serde_json::json!(["switch","-c","new-branch"]))});close_menu();}, config),
                    simple_menu_item(
                        "Checkout Tag or Revision...",
                        move || {workbench_command.send(SparrowWorkbenchCommand::PaletteSCMReferences);close_menu();},
                        config,
                    ),
                    divider(6.0),
                    // Recent
                    section_label("Recent"),
                    dyn_stack(
                        move || {
                            let query = search_text.get();
                            let current = current_branch.get();
                            if query.is_empty()
                                || current.to_lowercase().contains(&query)
                            {
                                vec![current]
                            } else {
                                vec![]
                            }
                        },
                        |name| name.clone(),
                        move |name| {
                            let is_current = name == current_branch.get_untracked();
                            branch_row(name, is_current)
                        },
                    ),
                    // Local Branches
                    section_label("Local"),
                    dyn_stack(
                        move || {
                            let query = search_text.get();
                            let current = current_branch.get();
                            branches
                                .get()
                                .into_iter()
                                .filter(|b| {
                                    !b.contains('/')
                                        && b.to_lowercase().contains(&query)
                                })
                                .map(|b| {
                                    let is_cur = b == current;
                                    (b, is_cur)
                                })
                                .collect::<Vec<_>>()
                        },
                        |(name, _)| name.clone(),
                        move |(name, is_current)| branch_row(name, is_current),
                    ),
                    // Remote Branches
                    section_label("Remote"),
                    dyn_stack(
                        move || {
                            let query = search_text.get();
                            branches
                                .get()
                                .into_iter()
                                .filter(|b| {
                                    b.contains('/')
                                        && b.to_lowercase().contains(&query)
                                })
                                .map(|b| (b, false))
                                .collect::<Vec<_>>()
                        },
                        |(name, _)| name.clone(),
                        move |(name, is_current)| branch_row(name, is_current),
                    ),
                    // Tags
                    section_label("Tags"),
                    dyn_stack(
                        move || {
                            let query = search_text.get();
                            tags.get()
                                .into_iter()
                                .filter(|t| t.to_lowercase().contains(&query))
                                .map(|t| (t, false))
                                .collect::<Vec<_>>()
                        },
                        |(name, _)| name.clone(),
                        move |(name, is_current)| branch_row(name, is_current),
                    ),
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.max_height(600.0).width_full()),
        )
        .on_click_stop(|_| {}) // Prevent clicks inside menu from closing it
        .style(move |s| {
            let c = config.get();
            let pt = origin.get() - window_origin.get().to_vec2();
            let th = title_height.get();

            s.absolute()
                .margin_left(pt.x as f32)
                .margin_top(th as f32 + 2.0)
                .width(450.0)
                .padding(0.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(6.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(8.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .display(if visible.get() {
                Display::Flex
            } else {
                Display::None
            })
            .z_index(100)
    })
}

/// `/home/me/x` -> `~/x`
fn display_path(path: &Path) -> String {
    let full = path.to_string_lossy().to_string();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if !home.is_empty() && full.starts_with(&home) {
        format!("~{}", &full[home.len()..])
    } else {
        full
    }
}

/// Current branch from `.git/HEAD`. Empty for detached HEAD, worktrees or non-repos.
fn read_git_branch(root: &Path) -> String {
    std::fs::read_to_string(root.join(".git/HEAD"))
        .ok()
        .and_then(|h| {
            h.trim()
                .strip_prefix("ref: refs/heads/")
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Menu row with a fixed icon column so all labels line up (like the reference).
fn menu_row(
    icon: Option<&'static str>,
    text: &'static str,
    on_click: impl Fn() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        container(
            svg(move || icon.map(|i| config.get().ui_svg(i)).unwrap_or_default())
                .style(move |s| {
                    let c = config.get();
                    let size = c.ui.icon_size() as f32;
                    s.size(size, size)
                        .color(c.color(SparrowColor::SPARROW_ICON_ACTIVE))
                }),
        )
        .style(|s| s.width(24.0).items_center()),
        label(move || text.to_string()).style(move |s| {
            s.font_size(13.0)
                .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
        }),
    ))
    .on_click_stop(move |_| on_click())
    .style(move |s| {
        let c = config.get();
        s.width_full()
            .items_center()
            .padding_horiz(8.0)
            .padding_vert(6.0)
            .border_radius(6.0)
            .hover(|s| {
                s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                    .cursor(CursorStyle::Pointer)
            })
    })
}

pub fn device_selector_overlay(
    window_tab_data: Rc<WindowTabData>,
    origin: RwSignal<Point>,
) -> impl View {
    let visible = window_tab_data.device_menu_visible;
    let config = window_tab_data.common.config;
    let hot_reload = window_tab_data.flutter_hot_reload.clone();

    let close_menu = {
        let visible = window_tab_data.device_menu_visible;
        move || visible.set(false)
    };

    let value = hot_reload.clone();
    let device_row = move |dev: FlutterDevice, is_current: bool| {
        let dev_clone = dev.clone();
        let hr = value.clone();
        let close = close_menu.clone();
        let config_get = config;

        let icon_key = crate::dart::hot_reload::device_platform_icon(
            dev.target_platform.as_deref(),
        );
        let platform_desc = dev
            .target_platform
            .clone()
            .unwrap_or_else(|| "desktop".to_string());

        container(
            stack((
                // Icon
                svg(move || {
                    let c = config_get.get();
                    c.ui_svg(icon_key)
                })
                .style(move |s| {
                    let c = config_get.get();
                    let size = c.ui.icon_size() as f32;
                    s.size(size, size).margin_right(10.0).color(if is_current {
                        c.color(SparrowColor::EDITOR_FOREGROUND)
                    } else {
                        c.color(SparrowColor::EDITOR_DIM)
                    })
                }),
                // Device Name & target platform
                stack((
                    label(move || dev_clone.name.clone()).style(move |s| {
                        let c = config_get.get();
                        s.font_size(13.0)
                            .font_weight(if is_current {
                                Weight::BOLD
                            } else {
                                Weight::NORMAL
                            })
                            .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                    }),
                    label(move || format!("• {}", platform_desc)).style(move |s| {
                        let c = config_get.get();
                        s.font_size(11.0)
                            .margin_left(6.0)
                            .color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                    }),
                ))
                .style(|s| s.items_center().flex_row().flex_grow(1.0_f32)),
                // Checkmark if current
                label(move || if is_current { "✓" } else { "" }).style(move |s| {
                    let c = config_get.get();
                    s.font_size(13.0)
                        .font_weight(Weight::BOLD)
                        .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                }),
            ))
            .style(|s| s.items_center().width_full()),
        )
        .on_click_stop(move |_| {
            hr.selected_device.set(Some(dev.clone()));
            close();
        })
        .style(move |s| {
            let c = config_get.get();
            s.width_full()
                .height(32.0)
                .padding_horiz(10.0)
                .items_center()
                .apply_if(is_current, |s| {
                    s.background(
                        c.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND)
                            .multiply_alpha(0.25),
                    )
                })
                .hover(|s| {
                    s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                        .cursor(CursorStyle::Pointer)
                })
        })
    };

    let emu_value = hot_reload.clone();
    let emulator_row = move |emu: crate::dart::hot_reload::FlutterEmulator| {
        let emu_clone = emu.clone();
        let hr = emu_value.clone();
        let close = close_menu.clone();
        let config_get = config;

        container(
            stack((
                svg(move || config_get.get().ui_svg(SparrowIcons::DEVICE_MOBILE))
                    .style(move |s| {
                        let c = config_get.get();
                        let size = c.ui.icon_size() as f32;
                        s.size(size, size)
                            .margin_right(10.0)
                            .color(c.color(SparrowColor::EDITOR_DIM))
                    }),
                label(move || emu_clone.name.clone()).style(move |s| {
                    let c = config_get.get();
                    s.font_size(13.0)
                        .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                        .flex_grow(1.0_f32)
                }),
                label(|| "Launch".to_string()).style(move |s| {
                    let c = config_get.get();
                    s.font_size(11.0).font_weight(Weight::BOLD).color(
                        c.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND),
                    )
                }),
            ))
            .style(|s| s.items_center().width_full()),
        )
        .on_click_stop(move |_| {
            hr.launch_emulator(&emu.id);
            close();
        })
        .style(move |s| {
            let c = config_get.get();
            s.width_full()
                .height(32.0)
                .padding_horiz(10.0)
                .items_center()
                .hover(|s| {
                    s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                        .cursor(CursorStyle::Pointer)
                })
        })
    };

    stack((
        // Background to catch outside clicks
        empty()
            .on_click_stop(move |_| close_menu())
            .style(|s| s.absolute().size_full()),
        // Menu Panel
        container(
            scroll(
                stack((
                    // Section header: Running Devices
                    label(|| "Running Devices".to_string()).style(move |s| {
                        let c = config.get();
                        s.font_size(11.0)
                            .font_weight(Weight::BOLD)
                            .padding_horiz(10.0)
                            .padding_vert(4.0)
                            .color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                    }),
                    // Device List
                    dyn_stack(
                        move || hot_reload.devices.get(),
                        |dev| dev.id.clone(),
                        move |dev| {
                            let is_current = hot_reload
                                .selected_device
                                .get_untracked()
                                .map(|d| d.id == dev.id)
                                .unwrap_or(false);
                            device_row(dev, is_current)
                        },
                    )
                    .style(|s| s.flex_col().width_full()),
                    // If no devices found
                    label(|| "No running devices detected".to_string()).style({
                        let hr = hot_reload.clone();
                        move |s| {
                            let c = config.get();
                            let is_empty = hr.devices.get().is_empty();
                            s.font_size(12.0)
                                .padding_horiz(10.0)
                                .padding_vert(4.0)
                                .color(c.color(SparrowColor::EDITOR_DIM))
                                .apply_if(!is_empty, |s| s.hide())
                        }
                    }),
                    // Emulators section (if any available)
                    stack((
                        empty().style(move |s| {
                            s.height(1.0).margin_vert(6.0).background(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                        }),
                        label(|| "Available Emulators".to_string()).style(
                            move |s| {
                                let c = config.get();
                                s.font_size(11.0)
                                    .font_weight(Weight::BOLD)
                                    .padding_horiz(10.0)
                                    .padding_vert(4.0)
                                    .color(
                                        c.color(SparrowColor::PANEL_FOREGROUND_DIM),
                                    )
                            },
                        ),
                        dyn_stack(
                            {
                                let hr = hot_reload.clone();
                                move || hr.emulators.get()
                            },
                            |emu| emu.id.clone(),
                            move |emu| emulator_row(emu),
                        )
                        .style(|s| s.flex_col().width_full()),
                    ))
                    .style({
                        let hr = hot_reload.clone();
                        move |s| {
                            let is_empty = hr.emulators.get().is_empty();
                            s.flex_col()
                                .width_full()
                                .apply_if(is_empty, |s| s.hide())
                        }
                    }),
                    // Divider
                    empty().style(move |s| {
                        s.height(1.0).margin_vert(6.0).background(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                    }),
                    // Refresh Devices Action
                    container(
                        stack((
                            svg(move || {
                                config
                                    .get()
                                    .ui_svg(SparrowIcons::FLUTTER_HOT_RESTART)
                            })
                            .style(move |s| {
                                let c = config.get();
                                let size = c.ui.icon_size() as f32;
                                s.size(size, size)
                                    .margin_right(10.0)
                                    .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                            }),
                            label(|| "Refresh Devices".to_string()).style(
                                move |s| {
                                    s.font_size(13.0).color(
                                        config
                                            .get()
                                            .color(SparrowColor::EDITOR_FOREGROUND),
                                    )
                                },
                            ),
                        ))
                        .style(|s| s.items_center()),
                    )
                    .on_click_stop({
                        let hr = hot_reload.clone();
                        let close = close_menu.clone();
                        move |_| {
                            hr.fetch_devices();
                            close();
                        }
                    })
                    .style(move |s| {
                        let c = config.get();
                        s.width_full()
                            .height(32.0)
                            .padding_horiz(10.0)
                            .items_center()
                            .hover(|s| {
                                s.background(
                                    c.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                                )
                                .cursor(CursorStyle::Pointer)
                            })
                    }),
                    // Restart Flutter Daemon Action
                    container(
                        stack((
                            svg(move || config.get().ui_svg(SparrowIcons::FLUTTER))
                                .style(move |s| {
                                    let c = config.get();
                                    let size = c.ui.icon_size() as f32;
                                    s.size(size, size).margin_right(10.0).color(
                                        c.color(SparrowColor::EDITOR_FOREGROUND),
                                    )
                                }),
                            label(|| "Restart Flutter Daemon".to_string()).style(
                                move |s| {
                                    s.font_size(13.0).color(
                                        config
                                            .get()
                                            .color(SparrowColor::EDITOR_FOREGROUND),
                                    )
                                },
                            ),
                        ))
                        .style(|s| s.items_center()),
                    )
                    .on_click_stop(move |_| {
                        hot_reload.restart_daemon();
                        close_menu();
                    })
                    .style(move |s| {
                        let c = config.get();
                        s.width_full()
                            .height(32.0)
                            .padding_horiz(10.0)
                            .items_center()
                            .hover(|s| {
                                s.background(
                                    c.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                                )
                                .cursor(CursorStyle::Pointer)
                            })
                    }),
                ))
                .style(|s| s.flex_col().width_full().padding_vert(4.0)),
            )
            .style(|s| s.max_height(400.0).width_full()),
        )
        .on_click_stop(|_| {}) // Prevent clicks inside menu from closing it
        .style(move |s| {
            let c = config.get();
            let pt = origin.get();
            let th = window_tab_data.title_height.get();

            s.absolute()
                .margin_left(pt.x as f32)
                // Anchor perfectly below the title bar, offset slightly to align with the chip
                .margin_top(th as f32 + 2.0)
                .width(300.0)
                .padding(4.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(6.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(8.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .display(if visible.get() {
                Display::Flex
            } else {
                Display::None
            })
            .z_index(100)
    })
}

fn project_initials(name: &str) -> String {
    let words: Vec<_> = name
        .split(|c: char| c == '_' || c == '-' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect();
    let mut initials = words
        .first()
        .and_then(|word| word.chars().next())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "S".into());
    if words.len() > 1 {
        if let Some(last) = words.last().and_then(|word| word.chars().next()) {
            initials.extend(last.to_uppercase());
        }
    }
    initials
}
fn device_display_name(device: &FlutterDevice) -> String {
    match device.target_platform.as_deref() {
        Some(platform)
            if platform.starts_with("linux")
                || platform.starts_with("darwin")
                || platform.starts_with("windows") =>
        {
            format!("{} (desktop)", device.name)
        }
        Some(platform) if platform.starts_with("web") => {
            format!("{} (web)", device.name)
        }
        _ => device.name.clone(),
    }
}

#[cfg(test)]
mod toolbar_tests {
    use super::*;
    #[test]
    fn project_initials_match_name_segments() {
        assert_eq!(project_initials("liquid_glass_kit"), "LK");
        assert_eq!(project_initials("app"), "A");
        assert_eq!(project_initials(""), "S");
    }
}
