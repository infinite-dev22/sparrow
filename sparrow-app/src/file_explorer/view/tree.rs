//! Virtualized Project tree, row appearance and pointer interactions.

use std::{rc::Rc, sync::Arc};

use floem::{
    View,
    event::{Event, EventListener},
    kurbo::Rect,
    peniko::Color,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    style::{AlignItems, CursorStyle, Style},
    views::{Decorators, container, label, stack, svg, virtual_stack},
};
use sparrow_rpc::{
    file::{FileNodeViewData, FileNodeViewKind, Naming},
    source_control::FileDiffKind,
};

use crate::file_explorer::{
    data::FileExplorerData, node::FileNodeVirtualList, project,
};
use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    panel::kind::PanelKind,
    source_control::SourceControlData,
    window_tab::{Focus, WindowTabData},
};
use crate::widgets::sparrow_scroll;
use super::naming::{
    file_node_input_view, initialize_naming_editor,
    initialize_naming_editor_with_path,
};

fn file_node_text_color(
    config: ReadSignal<Arc<SparrowConfig>>,
    node: FileNodeViewData,
    source_control: SourceControlData,
) -> Color {
    let diff = source_control.file_diffs.with(|file_diffs| {
        let FileNodeViewKind::Path(path) = &node.kind else {
            return None;
        };

        if node.is_dir {
            None
        } else {
            file_diffs.get(path).map(|(diff, _)| diff.kind())
        }
    });

    let color = match diff {
        Some(FileDiffKind::Modified | FileDiffKind::Renamed) => {
            SparrowColor::SOURCE_CONTROL_MODIFIED
        }
        Some(FileDiffKind::Added) => SparrowColor::SOURCE_CONTROL_ADDED,
        Some(FileDiffKind::Deleted) => SparrowColor::SOURCE_CONTROL_REMOVED,
        None => {
            if node.kind.path().and_then(|path| {
                project::role(
                    path,
                    source_control.common.workspace.path.as_deref(),
                )
            }) == Some("excluded")
            {
                return Color::from_rgb8(184, 133, 88);
            }
            SparrowColor::PANEL_FOREGROUND
        }
    };

    config.get().color(color)
}

fn file_node_text_view(
    data: FileExplorerData,
    node: FileNodeViewData,
    source_control: SourceControlData,
    project_info: RwSignal<project::ProjectInfo>,
) -> impl View {
    let config = data.common.config;
    let ui_line_height = data.common.ui_line_height;

    match node.kind.clone() {
        FileNodeViewKind::Path(path) => {
            if node.is_root {
                let file = path.clone();
                container((
                    label(move || {
                        file.file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default()
                    })
                    .style(move |s| {
                        s.height(ui_line_height.get())
                            .items_center()
                            .font_weight(floem::text::Weight(600))
                            .color(file_node_text_color(
                                config,
                                node.clone(),
                                source_control.clone(),
                            ))
                            .padding_right(5.0)
                            .selectable(false)
                    }),
                    label(move || {
                        let home =
                            std::env::var_os("HOME").map(std::path::PathBuf::from);
                        home.and_then(|home| {
                            path.strip_prefix(home)
                                .ok()
                                .map(|p| format!("~/{}", p.display()))
                        })
                        .unwrap_or_else(|| path.display().to_string())
                    })
                    .style(move |s| {
                        s.height(ui_line_height.get())
                            .color(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_FOREGROUND_DIM),
                            )
                            .selectable(false)
                    }),
                ))
            } else {
                let platform = node.is_dir
                    && path.parent() == data.common.workspace.path.as_deref()
                    && project::role(
                        &path,
                        data.common.workspace.path.as_deref(),
                    ) == Some("platform");
                container(
                    label(move || {
                        project_info
                            .with(|info| project::node_label(&path, info))
                    })
                    .style(move |s| {
                        s.height(ui_line_height.get())
                            .items_center()
                            .apply_if(platform, |s| {
                                s.font_weight(floem::text::Weight(600))
                            })
                            .color(file_node_text_color(
                                config,
                                node.clone(),
                                source_control.clone(),
                            ))
                            .selectable(false)
                    }),
                )
            }
        }
        FileNodeViewKind::Renaming { path, err } => {
            if data.naming.with_untracked(Naming::editor_needs_reset) {
                initialize_naming_editor_with_path(&data, &path);
            }

            file_node_input_view(data, err.clone())
        }
        FileNodeViewKind::Naming { err } => {
            if data.naming.with_untracked(Naming::editor_needs_reset) {
                initialize_naming_editor(&data, "", None);
            }

            file_node_input_view(data, err.clone())
        }
        FileNodeViewKind::Duplicating { source, err } => {
            if data.naming.with_untracked(Naming::editor_needs_reset) {
                initialize_naming_editor_with_path(&data, &source);
            }

            file_node_input_view(data, err.clone())
        }
    }
}

pub(super) fn file_explorer_view(
    data: FileExplorerData,
    source_control: SourceControlData,
    project_info: RwSignal<project::ProjectInfo>,
    window: Rc<WindowTabData>,
    library_watch: project::LibraryWatch,
) -> impl View {
    let root = data.root;
    let ui_line_height = data.common.ui_line_height;
    let config = data.common.config;
    let naming = data.naming;
    let scroll_to_line = data.scroll_to_line;
    let select = data.select;
    let secondary_click_data = data.clone();
    let scroll_rect = create_rw_signal(Rect::ZERO);
    let focus = data.common.focus;

    let external =
        project::external_libraries(window, project_info, library_watch);
    let workspace_root = data.common.workspace.path.clone();
    sparrow_scroll(
        stack((
            container(
                virtual_stack(
                    move || FileNodeVirtualList::new(root.get(), data.naming.get()),
                    move |node| {
                        (node.kind.clone(), node.is_dir, node.open, node.level)
                    },
                    move |node| {
                        let level = node.level;
                        let data = data.clone();
                        let click_data = data.clone();
                        let double_click_data = data.clone();
                        let secondary_click_data = data.clone();
                        let aux_click_data = data.clone();
                        let kind = node.kind.clone();
                        let open = node.open;
                        let is_dir = node.is_dir;
                        let is_root = node.is_root;
                        let role = kind.path().and_then(|path| {
                            project::role(path, workspace_root.as_deref())
                        });

                        let view = stack((
                            svg(move || {
                                let config = config.get();
                                let svg_str = match open {
                                    true => SparrowIcons::ITEM_OPENED,
                                    false => SparrowIcons::ITEM_CLOSED,
                                };
                                config.ui_svg(svg_str)
                            })
                            .style(move |s| {
                                let config = config.get();
                                let size = config.ui.icon_size() as f32;

                                s.size(size, size)
                                    .flex_shrink(0.0f32)
                                    .margin_left(10.0)
                                    .color(if is_dir {
                                        config
                                            .color(SparrowColor::SPARROW_ICON_ACTIVE)
                                    } else {
                                        Color::TRANSPARENT
                                    })
                            }),
                            {
                                let kind = kind.clone();
                                let kind_for_style = kind.clone();
                                // TODO: use the current naming input as the path for the file svg
                                svg(move || {
                                    let config = config.get();
                                    if is_root && project_info.with(|p| p.flutter) {
                                        config.ui_svg(SparrowIcons::FLUTTER)
                                    } else if is_dir {
                                        kind.path()
                                            .map(|path| config.folder_svg(path).0)
                                            .unwrap_or_else(|| {
                                                config
                                                    .ui_svg(SparrowIcons::DIRECTORY)
                                            })
                                    } else if let Some(path) = kind.path() {
                                        config.file_svg(path).0
                                    } else {
                                        config.ui_svg(SparrowIcons::FILE)
                                    }
                                })
                                .style(move |s| {
                                    let config = config.get();
                                    let size = config.ui.icon_size() as f32;

                                    s.size(size, size)
                                        .flex_shrink(0.0f32)
                                        .margin_horiz(6.0)
                                        .apply_opt(
                                            kind_for_style.path().and_then(|path| {
                                                if is_root
                                                    && project_info
                                                        .with(|p| p.flutter)
                                                {
                                                    None
                                                } else if is_dir
                                                    && role == Some("source")
                                                {
                                                    Some(config.color(
                                                        SparrowColor::EDITOR_LINK,
                                                    ))
                                                } else if is_dir
                                                    && role == Some("test")
                                                {
                                                    Some(Color::from_rgb8(
                                                        109, 161, 107,
                                                    ))
                                                } else if is_dir
                                                    && role == Some("excluded")
                                                {
                                                    Some(Color::from_rgb8(
                                                        184, 133, 88,
                                                    ))
                                                } else if is_dir {
                                                    config.folder_svg(path).1
                                                } else {
                                                    config.file_svg(path).1
                                                }
                                            }),
                                            Style::color,
                                        )
                                })
                            },
                            file_node_text_view(
                                data,
                                node,
                                source_control.clone(),
                                project_info,
                            ),
                        ))
                        .style({
                            let kind = kind.clone();
                            move |s| {
                                s.padding_right(15.0)
                                    .border_radius(6.0)
                                    .min_width_full()
                                    .padding_left((level * 10) as f32)
                                    .align_items(AlignItems::Center)
                                    .apply_if(role == Some("excluded"), |s| {
                                        s.background(Color::from_rgba8(
                                            122, 79, 40, 35,
                                        ))
                                    })
                                    .apply_if(role == Some("test"), |s| {
                                        s.background(Color::from_rgba8(
                                            58, 103, 48, 30,
                                        ))
                                    })
                                    .hover(|s| {
                                        s.background(config.get().color(
                                            SparrowColor::PANEL_HOVERED_BACKGROUND,
                                        ))
                                        .cursor(CursorStyle::Pointer)
                                    })
                                    .apply_if(
                                        select
                                            .get()
                                            .map(|x| x == kind)
                                            .unwrap_or_default(),
                                        |x| {
                                            x.background(config.get().color(
                                            SparrowColor::PANEL_SELECTION_BACKGROUND,
                                        ))
                                        },
                                    )
                            }
                        })
                        .debug_name("file item");

                        // Only handle click events if we are not naming the file node
                        if let FileNodeViewKind::Path(path) = &kind {
                            let click_path = path.clone();
                            let double_click_path = path.clone();
                            let secondary_click_path = path.clone();
                            let aux_click_path = path.clone();
                            view.on_click_stop({
                                let kind = kind.clone();
                                move |_| {
                                    click_data.click(&click_path, config);
                                    select.update(|x| *x = Some(kind.clone()));
                                    focus.set(Focus::Panel(PanelKind::FileExplorer));
                                }
                            })
                            .on_double_click({
                                move |_| {
                                    double_click_data
                                        .double_click(&double_click_path, config)
                                }
                            })
                            .on_secondary_click_stop(move |_| {
                                secondary_click_data
                                    .secondary_click(&secondary_click_path);
                            })
                            .on_event_stop(
                                EventListener::PointerDown,
                                move |event| {
                                    if let Event::PointerDown(pointer_event) = event
                                    {
                                        if pointer_event.button.is_auxiliary() {
                                            aux_click_data
                                                .middle_click(&aux_click_path);
                                        }
                                    }
                                },
                            )
                        } else {
                            view
                        }
                    },
                )
                .item_size_fixed(move || ui_line_height.get())
                .style(|s| s.absolute().flex_col().min_width_full()),
            )
            .style(move |s| {
                s.height(
                    (root.with(|r| r.children_open_count + 1) as f64
                        * ui_line_height.get()) as f32,
                )
                .min_width_full()
            }),
            external,
        ))
        .style(|s| s.flex_col().min_width_full()),
    )
    .style(|s| s.absolute().size_full().line_height(1.0))
    .on_secondary_click_stop(move |_| {
        if let Naming::None = naming.get_untracked() {
            if let Some(path) = &secondary_click_data.common.workspace.path {
                secondary_click_data.secondary_click(path);
            }
        }
    })
    .on_resize(move |rect| {
        scroll_rect.set(rect);
    })
    .scroll_to(move || {
        if let Some(line) = scroll_to_line.get() {
            let line_height = ui_line_height.get_untracked();
            Some(
                (
                    0.0,
                    line * line_height - scroll_rect.get_untracked().height() / 2.0,
                )
                    .into(),
            )
        } else {
            None
        }
    })
}
