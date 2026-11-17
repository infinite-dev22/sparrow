use crate::popup_menu::show_context_menu;
use crate::popup_menu::{Menu, MenuItem};
use std::{path::PathBuf, rc::Rc};

use floem::{
    View,
    event::{Event, EventListener},
    peniko::kurbo::Rect,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    style::{CursorStyle, Style},
    views::{Decorators, container, dyn_stack, label, scroll, stack, svg},
};
use sparrow_core::buffer::rope_text::RopeText;
use sparrow_rpc::source_control::FileDiff;

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    command::{
        CommandKind, InternalCommand, SparrowCommand, SparrowWorkbenchCommand,
    },
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::view::editor_view,
    settings::checkbox,
    source_control::SourceControlData,
    window_tab::{Focus, WindowTabData},
};

pub fn source_control_panel(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let source = window.source_control.clone();
    let config = window.common.config;
    let focus = source.common.focus;
    let editor = create_rw_signal(source.editor.clone());
    let doc = source.editor.doc_signal();
    let viewport = source.editor.viewport();
    let origin = source.editor.window_origin();
    let debug = create_memo(move |_| None);
    let active = move |tracked| {
        if tracked {
            focus.get() == Focus::Panel(PanelKind::SourceControl)
        } else {
            focus.get_untracked() == Focus::Panel(PanelKind::SourceControl)
        }
    };
    let message_width = create_rw_signal(400.0f64);
    let empty_message =
        create_memo(move |_| doc.get().buffer.with(|b| b.is_empty()));
    let commit = source.clone();
    let push = source.clone();
    let amend_source = source.clone();
    let amend = source.amend;
    let busy = source.busy;
    let status = source.status;
    let untracked = source.untracked;
    let files = source.file_diffs;
    let tracked_open = create_rw_signal(true);
    let new_open = create_rw_signal(true);
    let refresh_source = source.clone();
    let group = move |unversioned: bool, open: floem::reactive::RwSignal<bool>| {
        let count = move || {
            files.with(|items| {
                items
                    .values()
                    .filter(|(diff, _)| {
                        untracked.with(|set| set.contains(diff.path()))
                            == unversioned
                    })
                    .count()
            })
        };
        stack((
            stack((
                label(move || if open.get() { "⌄" } else { "›" })
                    .on_click_stop(move |_| open.update(|v| *v = !*v))
                    .style(|s| s.width(18.0)),
                checkbox(
                    move || {
                        files.with(|items| {
                            items
                                .values()
                                .filter(|(diff, _)| {
                                    untracked.with(|set| set.contains(diff.path()))
                                        == unversioned
                                })
                                .all(|(_, checked)| *checked)
                        })
                    },
                    config,
                )
                .on_click_stop(move |_| {
                    files.update(|items| {
                        let all = items
                            .values()
                            .filter(|(diff, _)| {
                                untracked.with(|set| set.contains(diff.path()))
                                    == unversioned
                            })
                            .all(|(_, checked)| *checked);
                        for (diff, checked) in items.values_mut() {
                            if untracked.with(|set| set.contains(diff.path()))
                                == unversioned
                            {
                                *checked = !all;
                            }
                        }
                    })
                }),
                label(move || {
                    format!(
                        "{}  {} files",
                        if unversioned {
                            "Unversioned Files"
                        } else {
                            "Changes"
                        },
                        count()
                    )
                })
                .style(|s| s.margin_left(6.0)),
            ))
            .style(move |s| {
                s.height(24.0)
                    .width_full()
                    .items_center()
                    .padding_horiz(6.0)
                    .background(
                        config
                            .get()
                            .color(SparrowColor::PANEL_HOVERED_BACKGROUND)
                            .with_alpha(0.3),
                    )
            }),
            file_diffs_view(source.clone(), unversioned).style(move |s| {
                s.min_height(0.0)
                    .height((count() * 24) as f32)
                    .apply_if(!open.get(), |s| s.hide())
            }),
        ))
        .style(move |s| {
            s.width_full()
                .flex_col()
                .apply_if(count() == 0, |s| s.hide())
        })
    };
    let message_view = stack((
        editor_view(editor.get_untracked(), debug, active)
            .style(move |s| s.width(message_width.get().max(40.0) as f32)),
        label(|| "Commit Message").style(move |s| {
            s.absolute()
                .color(config.get().color(SparrowColor::EDITOR_DIM))
                .apply_if(!empty_message.get(), |s| s.hide())
        }),
    ))
    .style(|s| s.absolute().min_size_pct(100.0, 100.0).padding(8.0));
    let message_id = message_view.id();
    let message_view = message_view
        .on_event_stop(EventListener::PointerDown, move |event| {
            focus.set(Focus::Panel(PanelKind::SourceControl));
            if let Event::PointerDown(pointer) = event.clone().offset((8.0, 8.0)) {
                message_id.request_active();
                editor.get_untracked().pointer_down(&pointer);
            }
        })
        .on_event_stop(EventListener::PointerMove, move |event| {
            if let Event::PointerMove(pointer) = event.clone().offset((8.0, 8.0)) {
                editor.get_untracked().pointer_move(&pointer);
            }
        })
        .on_event_stop(EventListener::PointerUp, move |event| {
            if let Event::PointerUp(pointer) = event.clone().offset((8.0, 8.0)) {
                editor.get_untracked().pointer_up(&pointer);
            }
        });
    stack((
        super::chrome::header(
            window.clone(),
            PanelKind::SourceControl,
            "Commit",
            floem::views::empty(),
        ),
        stack((
            crate::app::clickable_icon(
                || SparrowIcons::IMAGE_LOADING,
                {
                    let source = refresh_source.clone();
                    move || source.refresh()
                },
                || false,
                || false,
                || "Refresh changes",
                config,
            ),
            label(|| "Show Git Log")
                .on_click_stop(move |_| window.show_panel(PanelKind::GitHistory))
                .style(move |s| {
                    s.color(config.get().color(SparrowColor::EDITOR_LINK))
                }),
        ))
        .style(|s| s.height(28.0).gap(8.0).items_center().padding_horiz(6.0)),
        scroll(
            stack((group(false, tracked_open), group(true, new_open)))
                .style(|s| s.flex_col().width_full()),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(40.0).width_full()),
        stack((
            checkbox(move || amend.get(), config).on_click_stop(move |_| {
                amend_source.set_amend(!amend.get_untracked())
            }),
            label(|| "Amend last commit"),
        ))
        .style(move |s| {
            s.height(30.0)
                .gap(8.0)
                .padding_horiz(8.0)
                .items_center()
                .border_top(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        container(
            scroll(message_view)
                .on_move(move |point| origin.set(point + (8.0, 8.0)))
                .on_scroll(move |rect| viewport.set(rect))
                .style(|s| s.absolute().size_full()),
        )
        .on_resize(move |rect| message_width.set((rect.width() - 16.0).max(40.0)))
        .style(move |s| {
            s.height_pct(36.0)
                .min_height(100.0)
                .width_full()
                .border(1.0)
                .border_radius(2.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        label(move || status.get()).style(move |s| {
            s.width_full()
                .max_height(72.0)
                .padding_horiz(8.0)
                .font_size(11.0)
                .apply_if(status.with(|s| s.is_empty()), |s| s.hide())
        }),
        stack((
            label(move || {
                if busy.get() {
                    "Committing…"
                } else if amend.get() {
                    "Amend Commit"
                } else {
                    "Commit"
                }
            })
            .on_click_stop(move |_| commit.commit())
            .disabled(move || busy.get())
            .style(move |s| {
                s.padding_horiz(12.0)
                    .height(26.0)
                    .items_center()
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            }),
            label(|| "Commit and Push…")
                .on_click_stop(move |_| push.commit_with_options(true))
                .disabled(move || busy.get())
                .style(move |s| {
                    s.padding_horiz(12.0)
                        .height(26.0)
                        .items_center()
                        .border(1.0)
                        .border_radius(4.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                }),
        ))
        .style(|s| s.height(36.0).gap(6.0).items_center().padding_horiz(6.0)),
    ))
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
}

fn file_diffs_view(
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
            checkbox(move || checked, config).on_click_stop(move |_| {
                file_diffs.update(|diffs| {
                    if let Some((_, checked)) = diffs.get_mut(&full_path) {
                        *checked = !*checked;
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
        scroll({
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
