//! Palette result rows for the supported result types.

use std::sync::Arc;

use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    debug::RunDebugMode,
    keypress::keymap::KeyMap,
    palette::item::{PaletteItem, PaletteItemContent},
    widgets::focus_text::focus_text,
    workspace::SparrowWorkspace,
};
use floem::{
    View,
    reactive::{ReadSignal, SignalGet},
    style::{AlignItems, Style},
    views::{Decorators, container, dyn_stack, label, stack, svg},
};

pub(super) fn palette_item(
    workspace: Arc<SparrowWorkspace>,
    i: usize,
    item: PaletteItem,
    index: ReadSignal<usize>,
    palette_item_height: f64,
    config: ReadSignal<Arc<SparrowConfig>>,
    keymap: Option<&KeyMap>,
) -> impl View + use<> {
    match &item.content {
        PaletteItemContent::File { path, .. }
        | PaletteItemContent::Reference { path, .. } => {
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            // let (file_name, _) = create_signal(cx.scope, file_name);
            let folder = path
                .parent()
                .unwrap_or("".as_ref())
                .to_string_lossy()
                .into_owned();
            // let (folder, _) = create_signal(cx.scope, folder);
            let folder_len = folder.len();

            let file_name_indices = item
                .indices
                .iter()
                .filter_map(|&i| {
                    if folder_len > 0 {
                        if i > folder_len {
                            Some(i - folder_len - 1)
                        } else {
                            None
                        }
                    } else {
                        Some(i)
                    }
                })
                .collect::<Vec<_>>();
            let folder_indices = item
                .indices
                .iter()
                .filter_map(|&i| if i < folder_len { Some(i) } else { None })
                .collect::<Vec<_>>();

            let path = path.to_path_buf();
            let style_path = path.clone();
            container(
                stack((
                    svg(move || config.get().file_svg(&path).0).style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        let color = config.file_svg(&style_path).1;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .apply_opt(color, Style::color)
                    }),
                    focus_text(
                        move || file_name.clone(),
                        move || file_name_indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    focus_text(
                        move || folder.clone(),
                        move || folder_indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        s.color(config.get().color(SparrowColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0f32)
                            .flex_basis(0.0)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::DocumentSymbol {
            kind,
            name,
            container_name,
            ..
        } => {
            let kind = *kind;
            let text = name.to_string();
            let hint = container_name.clone().unwrap_or_default();
            let text_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i < text.len() { Some(i) } else { None }
                })
                .collect();
            let hint_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i >= text.len() {
                        Some(i - text.len())
                    } else {
                        None
                    }
                })
                .collect();
            container(
                stack((
                    svg(move || {
                        let config = config.get();
                        config
                            .symbol_svg(&kind)
                            .unwrap_or_else(|| config.ui_svg(SparrowIcons::FILE))
                    })
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.min_width(size).size(size, size).margin_right(5.0).color(
                            config.symbol_color(&kind).unwrap_or_else(|| {
                                config.color(SparrowColor::SPARROW_ICON_ACTIVE)
                            }),
                        )
                    }),
                    focus_text(
                        move || text.clone(),
                        move || text_indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    focus_text(
                        move || hint.clone(),
                        move || hint_indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        s.color(config.get().color(SparrowColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0f32)
                            .flex_basis(0.0)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::WorkspaceSymbol {
            kind,
            name,
            location,
            ..
        } => {
            let text = name.to_string();
            let kind = *kind;

            let path = location.path.clone();
            let full_path = location.path.clone();
            let path = if let Some(workspace_path) = workspace.path.as_ref() {
                path.strip_prefix(workspace_path)
                    .unwrap_or(&full_path)
                    .to_path_buf()
            } else {
                path
            };

            let hint = path.to_string_lossy().to_string();
            let text_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i < text.len() { Some(i) } else { None }
                })
                .collect();
            let hint_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i >= text.len() {
                        Some(i - text.len())
                    } else {
                        None
                    }
                })
                .collect();
            container(
                stack((
                    svg(move || {
                        let config = config.get();
                        config
                            .symbol_svg(&kind)
                            .unwrap_or_else(|| config.ui_svg(SparrowIcons::FILE))
                    })
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
                    }),
                    focus_text(
                        move || text.clone(),
                        move || text_indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    focus_text(
                        move || hint.clone(),
                        move || hint_indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        s.color(config.get().color(SparrowColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0f32)
                            .flex_basis(0.0)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::RunAndDebug {
            mode,
            config: run_config,
        } => {
            let mode = *mode;
            let text = format!("{mode} {}", run_config.name);
            let hint = format!(
                "{} {}",
                run_config.program,
                run_config.args.clone().unwrap_or_default().join(" ")
            );
            let text_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i < text.len() { Some(i) } else { None }
                })
                .collect();
            let hint_indices: Vec<usize> = item
                .indices
                .iter()
                .filter_map(|i| {
                    let i = *i;
                    if i >= text.len() {
                        Some(i - text.len())
                    } else {
                        None
                    }
                })
                .collect();
            container(
                stack((
                    svg(move || {
                        let config = config.get();
                        match mode {
                            RunDebugMode::Run => config.ui_svg(SparrowIcons::START),
                            RunDebugMode::Debug => {
                                config.ui_svg(SparrowIcons::DEBUG)
                            }
                        }
                    })
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
                    }),
                    focus_text(
                        move || text.clone(),
                        move || text_indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    focus_text(
                        move || hint.clone(),
                        move || hint_indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(move |s| {
                        s.color(config.get().color(SparrowColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0f32)
                            .flex_basis(0.0)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::PaletteHelp { .. }
        | PaletteItemContent::Command { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            let keys = if let Some(keymap) = keymap {
                keymap
                    .key
                    .iter()
                    .map(|key| key.label().trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            } else {
                vec![]
            };
            container(
                stack((
                    focus_text(
                        move || text.clone(),
                        move || indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(|s| {
                        s.flex_row()
                            .flex_grow(1.0f32)
                            .align_items(Some(AlignItems::Center))
                    }),
                    stack((dyn_stack(
                        move || keys.clone(),
                        |k| k.clone(),
                        move |key| {
                            label(move || key.clone()).style(move |s| {
                                s.padding_horiz(5.0)
                                    .padding_vert(1.0)
                                    .margin_right(5.0)
                                    .border(1.0)
                                    .border_radius(3.0)
                                    .border_color(
                                        config
                                            .get()
                                            .color(SparrowColor::SPARROW_BORDER),
                                    )
                                    .selectable(false)
                            })
                        },
                    ),)),
                ))
                .style(|s| s.width_full().items_center()),
            )
        }
        PaletteItemContent::Line { .. }
        | PaletteItemContent::Workspace { .. }
        | PaletteItemContent::SshHost { .. }
        | PaletteItemContent::Language { .. }
        | PaletteItemContent::LineEnding { .. }
        | PaletteItemContent::ColorTheme { .. }
        | PaletteItemContent::SCMReference { .. }
        | PaletteItemContent::TerminalProfile { .. }
        | PaletteItemContent::IconTheme { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            container(
                focus_text(
                    move || text.clone(),
                    move || indices.clone(),
                    move || config.get().color(SparrowColor::EDITOR_FOCUS),
                )
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::FileChooserEntry { is_dir, .. } => {
            let file_name = item.filter_text;
            let indices = item.indices;

            let (svg_path, svg_color) = (
                config.get().ui_svg(if *is_dir {
                    SparrowIcons::DIRECTORY
                } else {
                    SparrowIcons::FILE
                }),
                Some(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE)),
            );
            container(
                stack((
                    svg(move || svg_path.clone()).style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        let color = svg_color;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .apply_opt(color, Style::color)
                    }),
                    focus_text(
                        move || file_name.clone(),
                        move || indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::ConfirmFileChooserEntry { .. } => {
            let text = format!("Open {}", item.filter_text);
            let indices = item.indices;

            let (svg_path, svg_color) = (
                config.get().ui_svg(SparrowIcons::ADD),
                Some(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE)),
            );

            container(
                stack((
                    svg(move || svg_path.clone()).style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        let color = svg_color;
                        s.min_width(size)
                            .size(size, size)
                            .margin_right(5.0)
                            .apply_opt(color, Style::color)
                    }),
                    focus_text(
                        move || text.clone(),
                        move || indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        #[cfg(windows)]
        PaletteItemContent::WslHost { .. } => {
            let text = item.filter_text;
            let indices = item.indices;
            container(
                focus_text(
                    move || text.clone(),
                    move || indices.clone(),
                    move || config.get().color(SparrowColor::EDITOR_FOCUS),
                )
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
        PaletteItemContent::PubPackage {
            description,
            version,
            ..
        } => {
            let text = item.filter_text;
            let indices = item.indices;
            // Clone the borrowed fields so the closures can be `'static`.
            let description = description.clone();
            let version = version.clone();
            container(
                stack((
                    focus_text(
                        move || text.clone(),
                        move || indices.clone(),
                        move || config.get().color(SparrowColor::EDITOR_FOCUS),
                    )
                    .style(|s| s.margin_right(6.0).max_width_full()),
                    label(move || description.clone()).style(move |s| {
                        s.color(config.get().color(SparrowColor::EDITOR_DIM))
                            .min_width(0.0)
                            .flex_grow(1.0f32)
                            .flex_basis(0.0)
                            .text_ellipsis()
                    }),
                    label(move || version.clone()).style(move |s| {
                        s.color(config.get().color(SparrowColor::EDITOR_DIM))
                            .margin_left(6.0)
                            .selectable(false)
                    }),
                ))
                .style(|s| s.align_items(Some(AlignItems::Center)).max_width_full()),
            )
        }
    }
    .style(move |s| {
        s.width_full()
            .height(palette_item_height as f32)
            .padding_horiz(10.0)
            .apply_if(index.get() == i, |style| {
                style.background(
                    config.get().color(SparrowColor::PALETTE_CURRENT_BACKGROUND),
                )
            })
    })
}
