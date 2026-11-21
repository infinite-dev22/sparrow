//! Changed and unversioned file lists and their context menus.

use crate::widgets::popup_menu::show_context_menu;
use crate::widgets::popup_menu::{Menu, MenuItem};
use std::path::PathBuf;

use floem::{
    View,
    event::{Event, EventListener},
    peniko::kurbo::Rect,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    style::{CursorStyle, Style},
    views::{Decorators, container, dyn_stack, label, stack, svg},
};
use sparrow_rpc::source_control::FileDiff;

use crate::{
    command::{
        CommandKind, InternalCommand, SparrowCommand, SparrowWorkbenchCommand,
    },
    config::color::SparrowColor,
    source_control::SourceControlData,
    widgets::checkbox,
};
use crate::widgets::sparrow_scroll;

pub(super) fn file_diffs_view(
    source_control: SourceControlData,
    unversioned: bool,
) -> impl View {
    let file_diffs = source_control.file_diffs;
    let untracked = source_control.untracked;
    let config = source_control.common.config;
    let workspace = source_control.common.workspace.clone();
    let panel_rect = create_rw_signal(Rect::ZERO);
    let panel_width = create_memo(move |_| panel_rect.get().width());
    let sparrow_command = source_control.common.sparrow_command;
    let internal_command = source_control.common.internal_command;

    let view_fn = move |(path, (diff, checked)): (PathBuf, (FileDiff, bool))| {
        let full_path = path.clone();
        let diff_for_menu = diff.clone();
        let path_for_click = full_path.clone();

        let path = if let Some(workspace_path) = workspace.path.as_ref() {
            path.strip_prefix(workspace_path)
                .unwrap_or(&full_path)
                .to_path_buf()
        } else {
            path
        };
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let folder = path
            .parent()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let style_path = path.clone();
        stack((
            checkbox(move || checked, config).on_update(move |selected| {
                file_diffs.update(|diffs| {
                    if let Some((_, checked)) = diffs.get_mut(&full_path) {
                        *checked = selected;
                    }
                });
            }),
            svg(move || config.get().file_svg(&path).0).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let color = config.file_svg(&style_path).1;
                s.min_width(size)
                    .size(size, size)
                    .margin(6.0)
                    .apply_opt(color, Style::color)
            }),
            label(move || file_name.clone()).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let max_width = panel_width.get() as f32
                    - 10.0
                    - size
                    - 6.0
                    - size
                    - 6.0
                    - 10.0
                    - size
                    - 6.0;
                s.text_ellipsis()
                    .margin_right(6.0)
                    .max_width(max_width)
                    .selectable(false)
            }),
            label(move || folder.clone()).style(move |s| {
                s.text_ellipsis()
                    .flex_grow(1.0f32)
                    .flex_basis(0.0)
                    .color(config.get().color(SparrowColor::EDITOR_DIM))
                    .min_width(0.0)
                    .selectable(false)
            }),

        ))
        .on_click_stop(move |_| {
            internal_command.send(InternalCommand::OpenFileChanges {
                path: path_for_click.clone(),
            });
        })
        .on_event_cont(EventListener::PointerDown, move |event| {
            let diff_for_menu = diff_for_menu.clone();

            let discard = move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Workbench(
                        SparrowWorkbenchCommand::SourceControlDiscardTargetFileChanges,
                    ),
                    data: Some(serde_json::json!(diff_for_menu.clone())),
                });
            };

            if let Event::PointerDown(pointer_event) = event {
                if pointer_event.button.is_secondary() {
                    let menu = Menu::new("")
                        .entry(MenuItem::new("Discard Changes").action(discard));
                    show_context_menu(menu, None);
                }
            }
        })
        .style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.height(24.0).padding_left(28.0)
                .padding_right(10.0 + size + 6.0)
                .width_pct(100.0)
                .items_center()
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                })
        })
    };

    container({
        sparrow_scroll({
            dyn_stack(
                move || {
                    file_diffs
                        .get()
                        .into_iter()
                        .filter(|(_, (diff, _))| {
                            untracked.with(|set| set.contains(diff.path()))
                                == unversioned
                        })
                        .collect::<Vec<_>>()
                },
                |(path, (diff, checked))| {
                    (path.to_path_buf(), diff.clone(), *checked)
                },
                view_fn,
            )
            .style(|s| s.flex_col().width_pct(100.0))
        })
        .style(|s| s.absolute().size_pct(100.0, 100.0))
    })
    .on_resize(move |rect| {
        panel_rect.set(rect);
    })
    .style(|s| s.size_pct(100.0, 100.0))
}
