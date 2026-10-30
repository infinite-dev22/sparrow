use std::{
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
};

use floem::{
    View,
    reactive::{
        Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo,
    },
    style::{AlignItems, CursorStyle, Display},
    views::{Decorators, dyn_stack, label, stack, svg},
};
use indexmap::IndexMap;
use lsp_types::{DiagnosticSeverity, ProgressToken};
use sparrow_core::mode::{Mode, VisualMode};

use crate::{
    app::clickable_icon,
    command::SparrowWorkbenchCommand,
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    editor::EditorData,
    listener::Listener,
    palette::kind::PaletteKind,
    panel::{kind::PanelKind, position::PanelContainerPosition},
    source_control::SourceControlData,
    window_tab::{WindowTabData, WorkProgress},
};

#[allow(unused_variables)]
pub fn status(
    window_tab_data: Rc<WindowTabData>,
    source_control: SourceControlData,
    workbench_command: Listener<SparrowWorkbenchCommand>,
    status_height: RwSignal<f64>,
    _config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let config = window_tab_data.common.config;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let editor = window_tab_data.main_split.active_editor;
    let panel = window_tab_data.panel.clone();
    let palette = window_tab_data.palette.clone();
    let diagnostic_count = create_memo(move |_| {
        let mut errors = 0;
        let mut warnings = 0;
        for (_, diagnostics) in diagnostics.get().iter() {
            for diagnostic in diagnostics.diagnostics.get().iter() {
                if let Some(severity) = diagnostic.severity {
                    match severity {
                        DiagnosticSeverity::ERROR => errors += 1,
                        DiagnosticSeverity::WARNING => warnings += 1,
                        _ => (),
                    }
                }
            }
        }
        (errors, warnings)
    });
    let branch = source_control.branch;
    let file_diffs = source_control.file_diffs;
    let branch = move || {
        format!(
            "{}{}",
            branch.get(),
            if file_diffs.with(|diffs| diffs.is_empty()) {
                ""
            } else {
                "*"
            }
        )
    };

    let progresses = window_tab_data.progresses;
    let tab_data = window_tab_data.mode().clone();
    let mode = create_memo(move |_| tab_data);
    let pointer_down = floem::reactive::create_rw_signal(false);

    stack((
        stack((
            label(move || match mode.get() {
                Mode::Normal => "Normal".to_string(),
                Mode::Insert => "Insert".to_string(),
                Mode::Visual(mode) => match mode {
                    VisualMode::Normal => "Visual".to_string(),
                    VisualMode::Linewise => "Visual Line".to_string(),
                    VisualMode::Blockwise => "Visual Block".to_string(),
                },
                Mode::Terminal => "Terminal".to_string(),
            })
            .style(move |s| {
                let config = config.get();
                let display = if config.core.modal {
                    Display::Flex
                } else {
                    Display::None
                };

                let (bg, fg) = match mode.get() {
                    Mode::Normal => (
                        SparrowColor::STATUS_MODAL_NORMAL_BACKGROUND,
                        SparrowColor::STATUS_MODAL_NORMAL_FOREGROUND,
                    ),
                    Mode::Insert => (
                        SparrowColor::STATUS_MODAL_INSERT_BACKGROUND,
                        SparrowColor::STATUS_MODAL_INSERT_FOREGROUND,
                    ),
                    Mode::Visual(_) => (
                        SparrowColor::STATUS_MODAL_VISUAL_BACKGROUND,
                        SparrowColor::STATUS_MODAL_VISUAL_FOREGROUND,
                    ),
                    Mode::Terminal => (
                        SparrowColor::STATUS_MODAL_TERMINAL_BACKGROUND,
                        SparrowColor::STATUS_MODAL_TERMINAL_FOREGROUND,
                    ),
                };

                let bg = config.color(bg);
                let fg = config.color(fg);

                s.display(display)
                    .padding_horiz(10.0)
                    .color(fg)
                    .background(bg)
                    .height_pct(100.0)
                    .align_items(Some(AlignItems::Center))
                    .selectable(false)
            }),
            status_breadcrumbs(window_tab_data.workspace.clone(), editor, config),
            {
                let panel = panel.clone();
                stack((
                    svg(move || config.get().ui_svg(SparrowIcons::ERROR)).style(
                        move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size).color(
                                config.color(SparrowColor::SPARROW_ICON_ACTIVE),
                            )
                        },
                    ),
                    label(move || diagnostic_count.get().0.to_string()).style(
                        move |s| {
                            s.margin_left(5.0)
                                .color(
                                    config
                                        .get()
                                        .color(SparrowColor::STATUS_FOREGROUND),
                                )
                                .selectable(false)
                        },
                    ),
                    svg(move || config.get().ui_svg(SparrowIcons::WARNING)).style(
                        move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size).margin_left(5.0).color(
                                config.color(SparrowColor::SPARROW_ICON_ACTIVE),
                            )
                        },
                    ),
                    label(move || diagnostic_count.get().1.to_string()).style(
                        move |s| {
                            s.margin_left(5.0)
                                .color(
                                    config
                                        .get()
                                        .color(SparrowColor::STATUS_FOREGROUND),
                                )
                                .selectable(false)
                        },
                    ),
                ))
                .on_click_stop(move |_| {
                    panel.show_panel(&PanelKind::Problem);
                })
                .style(move |s| {
                    s.height_pct(100.0)
                        .padding_horiz(10.0)
                        .items_center()
                        .hover(|s| {
                            s.cursor(CursorStyle::Pointer).background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
            },
            progress_view(config, progresses),
        ))
        .style(|s| {
            s.height_pct(100.0)
                .min_width(0.0)
                .flex_basis(0.0)
                .flex_grow(1.0f32)
                .items_center()
        }),
        stack((
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel
                            .is_container_shown(&PanelContainerPosition::Left, true)
                        {
                            SparrowIcons::SIDEBAR_LEFT
                        } else {
                            SparrowIcons::SIDEBAR_LEFT_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel.toggle_container_visual(&PanelContainerPosition::Left)
                    },
                    || false,
                    || false,
                    || "Toggle Left Panel",
                    config,
                )
            },
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel.is_container_shown(
                            &PanelContainerPosition::Bottom,
                            true,
                        ) {
                            SparrowIcons::LAYOUT_PANEL
                        } else {
                            SparrowIcons::LAYOUT_PANEL_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel
                            .toggle_container_visual(&PanelContainerPosition::Bottom)
                    },
                    || false,
                    || false,
                    || "Toggle Bottom Panel",
                    config,
                )
            },
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel
                            .is_container_shown(&PanelContainerPosition::Right, true)
                        {
                            SparrowIcons::SIDEBAR_RIGHT
                        } else {
                            SparrowIcons::SIDEBAR_RIGHT_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel.toggle_container_visual(&PanelContainerPosition::Right)
                    },
                    || false,
                    || false,
                    || "Toggle Right Panel",
                    config,
                )
            },
        ))
        .style(move |s| {
            // Islands look: panel toggles live on the tool-window stripes.
            s.hide()
                .height_pct(100.0)
                .items_center()
                .color(config.get().color(SparrowColor::STATUS_FOREGROUND))
        }),
        stack({
            let palette_clone = palette.clone();
            let cursor_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let mut status = String::new();
                    let cursor = editor.cursor().get();
                    if let Some((line, column, _)) = editor
                        .doc_signal()
                        .get()
                        .buffer
                        .with(|buffer| cursor.get_line_col_char(buffer))
                    {
                        status = format!("{}:{}", line + 1, column + 1);
                    }
                    if let Some(selection) = cursor.get_selection() {
                        let selection_range = selection.0.abs_diff(selection.1);

                        if selection.0 != selection.1 {
                            status = format!("{status} ({selection_range} chars)");
                        }
                    }
                    let selection_count = cursor.get_selection_count();
                    if selection_count > 1 {
                        status = format!("{status} {selection_count} selections");
                    }
                    return status;
                }
                String::new()
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::Line);
            });
            let palette_clone = palette.clone();
            let line_ending_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let doc = editor.doc_signal().get();
                    doc.buffer.with(|b| b.line_ending()).as_str()
                } else {
                    ""
                }
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::LineEnding);
            });
            let palette_clone = palette.clone();
            let language_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let doc = editor.doc_signal().get();
                    doc.syntax().with(|s| s.language.name())
                } else {
                    "unknown"
                }
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::Language);
            });
            let encoding_info = status_text(config, editor, || "UTF-8");
            let flutter_button = flutter_run_button(window_tab_data, config);
            (
                flutter_button,
                cursor_info,
                line_ending_info,
                encoding_info,
                language_info,
            )
        })
        .style(|s| {
            s.height_pct(100.0)
                .flex_basis(0.0)
                .flex_grow(1.0f32)
                .justify_end()
        }),
    ))
    .on_resize(move |rect| {
        let height = rect.height();
        if height != status_height.get_untracked() {
            status_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.background(config.color(SparrowColor::STATUS_BACKGROUND))
            .flex_basis(config.ui.status_height() as f32)
            .flex_grow(0.0f32)
            .flex_shrink(0.0f32)
            .items_center()
    })
    .debug_name("Status/Bottom Bar")
}

/// Islands style breadcrumb trail: `project > dir > dir > file`.
fn status_breadcrumbs(
    workspace: Arc<crate::workspace::SparrowWorkspace>,
    editor: Memo<Option<EditorData>>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    use crate::doc::DocContent;
    dyn_stack(
        move || {
            let mut sections: Vec<String> = Vec::new();
            let Some(editor) = editor.get() else {
                return Vec::new().into_iter().enumerate();
            };
            let doc = editor.doc_signal().get();
            let content = doc.content.get();
            let full_path = if let DocContent::History(history) = &content {
                Some(history.path.clone())
            } else {
                content.path().cloned()
            };
            let Some(full_path) = full_path else {
                return Vec::new().into_iter().enumerate();
            };
            let mut path = full_path.clone();
            if let Some(workspace_path) = workspace.path.as_ref() {
                if let Ok(stripped) = full_path.strip_prefix(workspace_path) {
                    path = stripped.to_path_buf();
                    if let Some(root) = workspace_path.file_name() {
                        sections.push(root.to_string_lossy().into_owned());
                    }
                }
            }
            let mut parts: Vec<String> = path
                .ancestors()
                .filter_map(|p| Some(p.file_name()?.to_string_lossy().into_owned()))
                .collect();
            parts.reverse();
            sections.extend(parts);
            sections.into_iter().enumerate()
        },
        |(i, section)| (*i, section.clone()),
        move |(i, section)| {
            stack((
                svg(move || config.get().ui_svg(SparrowIcons::BREADCRUMB_SEPARATOR))
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.apply_if(i == 0, |s| s.hide())
                            .size(size, size)
                            .color(config.color(SparrowColor::SPARROW_ICON_INACTIVE))
                    }),
                label(move || section.clone())
                    .style(|s| s.selectable(false).margin_horiz(2.0)),
            ))
            .style(|s| s.items_center())
        },
    )
    .style(|s| s.padding_left(10.0).items_center())
}

fn progress_view(
    config: ReadSignal<Arc<SparrowConfig>>,
    progresses: RwSignal<IndexMap<ProgressToken, WorkProgress>>,
) -> impl View {
    let id = AtomicU64::new(0);
    dyn_stack(
        move || progresses.get(),
        move |_| id.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        move |(_, p)| {
            let progress = match p.message {
                Some(message) if !message.is_empty() => {
                    format!("{}: {}", p.title, message)
                }
                _ => p.title,
            };
            label(move || progress.clone()).style(move |s| {
                s.height_pct(100.0)
                    .min_width(0.0)
                    .margin_left(10.0)
                    .text_ellipsis()
                    .selectable(false)
                    .items_center()
                    .color(config.get().color(SparrowColor::STATUS_FOREGROUND))
            })
        },
    )
    .style(move |s| s.flex_row().height_pct(100.0).min_width(0.0))
}

fn status_text<S: std::fmt::Display + 'static>(
    config: ReadSignal<Arc<SparrowConfig>>,
    editor: Memo<Option<EditorData>>,
    text: impl Fn() -> S + 'static,
) -> impl View {
    label(text).style(move |s| {
        let config = config.get();
        let display = if editor
            .get()
            .map(|editor| {
                editor.doc_signal().get().content.with(|c| {
                    use crate::doc::DocContent;
                    matches!(c, DocContent::File { .. } | DocContent::Scratch { .. })
                })
            })
            .unwrap_or(false)
        {
            Display::Flex
        } else {
            Display::None
        };

        s.display(display)
            .height_full()
            .padding_horiz(10.0)
            .items_center()
            .color(config.color(SparrowColor::STATUS_FOREGROUND))
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
            .selectable(false)
    })
}

/// Flutter run / reload / restart status button.
///
/// Hidden entirely if the workspace is not a Flutter project (no
/// `pubspec.yaml` at the root). Otherwise shows the current state:
/// Idle → "Run Flutter", Running → "Reload", Error → "Retry".
fn flutter_run_button(
    window_tab_data: Rc<WindowTabData>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    use crate::{command::SparrowWorkbenchCommand, dart::FlutterRunState};
    use floem::{
        menu::{Menu, MenuItem},
        views::{container, label, svg},
    };

    let hot_reload = window_tab_data.flutter_hot_reload.clone();
    let workbench_command = window_tab_data.common.workbench_command;
    let is_flutter = hot_reload.is_flutter_project();

    let label_text = move || match hot_reload.state.get() {
        FlutterRunState::Idle => "Run Flutter",
        FlutterRunState::Running => "Reload",
        FlutterRunState::Error => "Retry",
    };
    let icon_name = move || match hot_reload.state.get() {
        FlutterRunState::Idle | FlutterRunState::Running => SparrowIcons::START,
        FlutterRunState::Error => SparrowIcons::RUN_ERRORS,
    };
    let icon_color = move || {
        let c = config.get();
        match hot_reload.state.get() {
            FlutterRunState::Idle | FlutterRunState::Running => {
                c.color(SparrowColor::SPARROW_RUN_ICON)
            }
            FlutterRunState::Error => c.color(SparrowColor::SPARROW_ERROR),
        }
    };

    container(
        stack((
            svg(move || config.get().ui_svg(icon_name())).style(move |s| {
                let c = config.get();
                let size = c.ui.icon_size() as f32;
                s.size(size, size).margin_right(5.0).color(icon_color())
            }),
            label(label_text).style(move |s| {
                s.color(config.get().color(SparrowColor::STATUS_FOREGROUND))
                    .selectable(false)
            }),
        ))
        .on_click_stop(move |_| match hot_reload.state.get_untracked() {
            FlutterRunState::Idle => {
                workbench_command.send(SparrowWorkbenchCommand::FlutterRun);
            }
            FlutterRunState::Running => {
                workbench_command.send(SparrowWorkbenchCommand::FlutterHotReload);
            }
            FlutterRunState::Error => {
                workbench_command.send(SparrowWorkbenchCommand::FlutterRun);
            }
        })
        .popout_menu(move || {
            let mut menu = Menu::new("");
            match hot_reload.state.get_untracked() {
                FlutterRunState::Idle | FlutterRunState::Error => {
                    menu =
                        menu.entry(MenuItem::new("Run Flutter").action(move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::FlutterRun);
                        }));
                }
                FlutterRunState::Running => {
                    menu = menu
                        .entry(MenuItem::new("Hot Reload").action(move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::FlutterHotReload);
                        }))
                        .entry(MenuItem::new("Hot Restart").action(move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::FlutterHotRestart);
                        }))
                        .separator()
                        .entry(MenuItem::new("Stop").action(move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::FlutterStop);
                        }));
                }
            }
            menu
        })
        .style(move |s| {
            let c = config.get();
            s.padding_horiz(10.0)
                .height_pct(100.0)
                .items_center()
                .cursor(CursorStyle::Pointer)
                .apply_if(!is_flutter, |s| s.hide())
                .hover(|s| {
                    s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                })
        })
        .debug_name("Flutter Run Button"),
    )
    .style(|s| s.height_pct(100.0))
}
