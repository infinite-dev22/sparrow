//! Debugger breakpoint list and gutter navigation.

use std::rc::Rc;

use floem::{
    View,
    event::EventListener,
    reactive::{SignalGet, SignalUpdate, create_rw_signal},
    views::{Decorators, container, dyn_stack, scroll, stack, text},
};

use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    widgets::checkbox,
    widgets::clickable_icon,
    window_tab::WindowTabData,
};

pub(in crate::panel) fn breakpoints_view(
    window_tab_data: Rc<WindowTabData>,
) -> impl View {
    let breakpoints = window_tab_data.terminal.debug.breakpoints;
    let config = window_tab_data.common.config;
    let workspace = window_tab_data.common.workspace.clone();
    let available_width = create_rw_signal(0.0);
    let internal_command = window_tab_data.common.internal_command;
    container(
        scroll(
            dyn_stack(
                move || {
                    breakpoints
                        .get()
                        .into_iter()
                        .flat_map(|(path, breakpoints)| {
                            breakpoints.into_values().map(move |b| (path.clone(), b))
                        })
                },
                move |(path, breakpoint)| {
                    (path.clone(), breakpoint.line, breakpoint.active)
                },
                move |(path, breakpoint)| {
                    let line = breakpoint.line;
                    let full_path = path.clone();
                    let full_path_for_jump = path.clone();
                    let full_path_for_close = path.clone();
                    let path = if let Some(workspace_path) = workspace.path.as_ref()
                    {
                        path.strip_prefix(workspace_path)
                            .unwrap_or(&full_path)
                            .to_path_buf()
                    } else {
                        path
                    };

                    let file_name =
                        path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    let folder =
                        path.parent().and_then(|s| s.to_str()).unwrap_or("");
                    let folder_empty = folder.is_empty();

                    stack((
                        clickable_icon(
                            move || SparrowIcons::CLOSE,
                            move || {
                                breakpoints.update(|breakpoints| {
                                    if let Some(breakpoints) =
                                        breakpoints.get_mut(&full_path_for_close)
                                    {
                                        breakpoints.remove(&line);
                                    }
                                });
                            },
                            || false,
                            || false,
                            || "Remove",
                            config,
                        )
                        .on_event_stop(EventListener::PointerDown, |_| {}),
                        checkbox(move || breakpoint.active, config)
                            .on_update(move |active| {
                                breakpoints.update(|breakpoints| {
                                    if let Some(breakpoints) =
                                        breakpoints.get_mut(&full_path)
                                    {
                                        if let Some(breakpoint) =
                                            breakpoints.get_mut(&line)
                                        {
                                            breakpoint.active = active;
                                        }
                                    }
                                });
                            })
                            .style(|s| s.margin_right(6.0)),
                        text(format!("{file_name}:{}", breakpoint.line + 1)).style(
                            move |s| {
                                let size = config.get().ui.icon_size() as f32;
                                s.text_ellipsis().max_width(
                                    available_width.get() as f32
                                        - 20.0
                                        - size
                                        - 6.0
                                        - size
                                        - 8.0,
                                )
                            },
                        ),
                        text(folder).style(move |s| {
                            s.text_ellipsis()
                                .flex_grow(1.0f32)
                                .flex_basis(0.0)
                                .color(config.get().color(SparrowColor::EDITOR_DIM))
                                .min_width(0.0)
                                .margin_left(6.0)
                                .apply_if(folder_empty, |s| s.hide())
                        }),
                    ))
                    .style(move |s| {
                        s.items_center().padding_horiz(10.0).width_pct(100.0).hover(
                            |s| {
                                s.background(
                                    config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            },
                        )
                    })
                    .on_click_stop(move |_| {
                        internal_command.send(
                            InternalCommand::DebugJumpToLocation {
                                location: EditorLocation {
                                    path: full_path_for_jump.clone(),
                                    position: Some(EditorPosition::Line(line)),
                                    scroll_offset: None,
                                    ignore_unconfirmed: false,
                                    same_editor_tab: false,
                                },
                            },
                        );
                    })
                },
            )
            .style(|s| s.flex_col().line_height(1.6).width_pct(100.0)),
        )
        .on_resize(move |rect| {
            let width = rect.width();
            if available_width.get_untracked() != width {
                available_width.set(width);
            }
        })
        .style(|s| s.absolute().size_pct(100.0, 100.0)),
    )
    .style(|s| s.size_pct(100.0, 100.0))
}
