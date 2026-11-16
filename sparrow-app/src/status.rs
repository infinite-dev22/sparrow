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
    app::tooltip_label,
    command::SparrowWorkbenchCommand,
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    editor::EditorData,
    listener::Listener,
    palette::kind::PaletteKind,
    panel::kind::PanelKind,
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
    let progresses = window_tab_data.progresses;
    let tab_data = window_tab_data.mode().clone();
    let mode = create_memo(move |_| tab_data);

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
            status_breadcrumbs(window_tab_data.clone(), editor, config),
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
                        .apply_if(diagnostic_count.get() == (0, 0), |s| s.hide())
                        .padding_horiz(8.0)
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
        ))
        .style(|s| {
            s.height_pct(100.0)
                .min_width(0.0)
                .flex_basis(0.0)
                .flex_grow(1.0f32)
                .items_center()
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
            let indentation_info = status_text(config, editor, move || {
                editor
                    .get()
                    .map(|editor| {
                        editor
                            .doc()
                            .buffer
                            .with(|buffer| buffer.indent_style().to_string())
                    })
                    .unwrap_or_default()
            })
            .popout_menu(move || {
                floem::menu::Menu::new("").entry(
                    floem::menu::MenuItem::new("Editor Settings...").action(
                        move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::OpenSettings)
                        },
                    ),
                )
            });
            let file_access = tooltip_label(
                config,
                svg(move || {
                    let read_only = editor.get().is_some_and(|editor| {
                        editor.doc_signal().get().content.with(|content| content.read_only())
                    });
                    config.get().ui_svg(if read_only {
                        "status.read-only"
                    } else {
                        "status.writable"
                    })
                })
                .style(move |s| {
                    s.size(12.0, 12.0)
                        .apply_if(editor.get().is_none(), |s| s.hide())
                        .margin_horiz(8.0)
                        .color(config.get().color(SparrowColor::STATUS_FOREGROUND))
                }),
                move || {
                    if editor.get().is_some_and(|editor| {
                        editor.doc_signal().get().content.with(|content| content.read_only())
                    }) {
                        "Read-only"
                    } else {
                        "Writable"
                    }
                },
            );
            let encoding_info = status_text(config, editor, || "UTF-8");
            (
                progress_view(config, progresses),
                cursor_info,
                line_ending_info,
                encoding_info,
                indentation_info,
                file_access,
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
        s.background(config.color(SparrowColor::SPARROW_WINDOW_BACKGROUND)) // Match title bar
            .flex_basis(config.ui.status_height() as f32)
            .flex_grow(0.0f32)
            .flex_shrink(0.0f32)
            .items_center()
    })
    .debug_name("Status/Bottom Bar")
}

/// Islands style breadcrumb trail: `project > dir > dir > file`.
fn status_breadcrumbs(
    window: Rc<WindowTabData>,
    editor: Memo<Option<EditorData>>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let workspace = window.workspace.clone();
    dyn_stack(
        move || {
            let Some(editor) = editor.get() else {
                return Vec::new().into_iter().enumerate();
            };
            // Preview tabs can change documents while retaining the editor ID.
            let doc = editor.doc_signal().get();
            let path = doc.content.with(|content| match content {
                crate::doc::DocContent::History(history) => {
                    Some(history.path.clone())
                }
                content => content.path().cloned(),
            });
            path.map(|path| breadcrumb_paths(workspace.path.as_deref(), &path))
                .unwrap_or_default()
                .into_iter()
                .enumerate()
        },
        |(i, path)| (*i, path.clone()),
        move |(i, path)| {
            let is_file = editor.get_untracked().is_some_and(|editor| {
                editor
                    .doc()
                    .content
                    .with_untracked(|content| content.path() == Some(&path))
            });
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let icon_path = path.clone();
            let icon_color_path = path.clone();
            let window = window.clone();
            stack((
                svg(move || config.get().ui_svg(SparrowIcons::BREADCRUMB_SEPARATOR))
                    .style(move |s| {
                        s.size(12.0, 12.0).apply_if(i == 0, |s| s.hide()).color(
                            config.get().color(SparrowColor::SPARROW_ICON_INACTIVE),
                        )
                    }),
                svg(move || config.get().files_svg(&[&icon_path]).0).style(
                    move |s| {
                        s.size(14.0, 14.0)
                            .margin_right(4.0)
                            .apply_if(!is_file, |s| s.hide())
                            .apply_opt(
                                config.get().files_svg(&[&icon_color_path]).1,
                                |s, color| s.color(color),
                            )
                    },
                ),
                label(move || name.clone())
                    .style(|s| s.selectable(false).margin_horiz(2.0)),
            ))
            .on_click_stop(move |_| {
                window.file_explorer.reveal_in_file_tree(path.clone());
                window.show_panel(PanelKind::FileExplorer);
            })
            .style(move |s| {
                s.items_center().cursor(CursorStyle::Pointer).hover(|s| {
                    s.background(
                        config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
            })
        },
    )
    .style(|s| s.padding_left(10.0).items_center().min_width(0.0))
}

fn breadcrumb_paths(
    root: Option<&std::path::Path>,
    file: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = file
        .ancestors()
        .take_while(|path| Some(*path) != root)
        .filter(|path| path.file_name().is_some())
        .map(std::path::Path::to_path_buf)
        .collect();
    if let Some(root) = root.filter(|root| file.starts_with(root)) {
        paths.push(root.to_path_buf());
    }
    paths.reverse();
    paths
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
            .padding_horiz(8.0)
            .items_center()
            .color(config.color(SparrowColor::STATUS_FOREGROUND))
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
            .selectable(false)
    })
}

#[cfg(test)]
mod breadcrumb_tests {
    use super::*;
    use std::path::{Path, PathBuf};
    #[test]
    fn workspace_crumbs_have_real_navigation_paths() {
        let paths = breadcrumb_paths(
            Some(Path::new("/work/app")),
            Path::new("/work/app/lib/my file.dart"),
        );
        assert_eq!(
            paths,
            ["/work/app", "/work/app/lib", "/work/app/lib/my file.dart"]
                .map(PathBuf::from)
        );
        let outside = breadcrumb_paths(
            Some(Path::new("/work/app")),
            Path::new("/work/app_other/main.dart"),
        );
        assert!(!outside.contains(&PathBuf::from("/work/app")));
    }
}
