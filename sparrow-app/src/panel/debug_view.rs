use std::rc::Rc;

use floem::{
    View,
    event::EventListener,
    peniko::Color,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    style::CursorStyle,
    views::{
        Decorators, container, dyn_stack, scroll, stack, svg, text, virtual_stack,
    },
};

use super::position::PanelPosition;
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    debug::DapVariable,
    editor::location::{EditorLocation, EditorPosition},
    settings::checkbox,
    window_tab::WindowTabData,
};

pub fn debug_panel(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    super::debug_session::panel(window_tab_data, position)
}

pub(super) fn inspector(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    super::debug_inspector::inspector(window_tab_data, position)
}

pub(super) fn variables_view(window_tab_data: Rc<WindowTabData>) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    let local_terminal = window_tab_data.terminal.clone();
    let ui_line_height = window_tab_data.common.ui_line_height;
    let config = window_tab_data.common.config;
    container(
        scroll(
            virtual_stack(
                move || {
                    let dap = terminal.get_active_dap(true);
                    dap.map(|dap| {
                        if !dap.stopped.get() {
                            return DapVariable::default();
                        }
                        let process_stopped = terminal
                            .get_terminal(&dap.term_id)
                            .and_then(|t| {
                                t.run_debug.with(|r| r.as_ref().map(|r| r.stopped))
                            })
                            .unwrap_or(
                                terminal.debug.active_dap.get() != Some(dap.dap_id),
                            );
                        if process_stopped {
                            return DapVariable::default();
                        }
                        let mut values = dap.variables.get();
                        if terminal.common.debugger_settings.get().sort_variables {
                            crate::debug_settings::sort_variables(&mut values);
                        }
                        values
                    })
                    .unwrap_or_default()
                },
                |node| {
                    (
                        node.item.name().to_string(),
                        node.item.value().map(|v| v.to_string()),
                        node.item.reference(),
                        node.expanded,
                        node.level,
                    )
                },
                move |node| {
                    let local_terminal = local_terminal.clone();
                    let level = node.level;
                    let reference = node.item.reference();
                    let name = node.item.name();
                    let ty = node.item.ty();
                    let type_exists = ty.map(|ty| !ty.is_empty()).unwrap_or(false);
                    let has_value = node.item.value().is_some();
                    stack((
                        svg(move || {
                            let config = config.get();
                            let svg_str = match node.expanded {
                                true => SparrowIcons::ITEM_OPENED,
                                false => SparrowIcons::ITEM_CLOSED,
                            };
                            config.ui_svg(svg_str)
                        })
                        .style(move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;

                            let color = if reference > 0 {
                                config.color(SparrowColor::SPARROW_ICON_ACTIVE)
                            } else {
                                Color::TRANSPARENT
                            };
                            s.size(size, size).margin_left(10.0).color(color)
                        }),
                        text(name),
                        text(": ").style(move |s| {
                            s.apply_if(!type_exists || reference == 0, |s| s.hide())
                        }),
                        text(node.item.ty().unwrap_or("")).style(move |s| {
                            s.color(config.get().style_color("type").unwrap())
                                .apply_if(!type_exists || reference == 0, |s| {
                                    s.hide()
                                })
                        }),
                        text(format!(" = {}", node.item.value().unwrap_or("")))
                            .style(move |s| s.apply_if(!has_value, |s| s.hide())),
                    ))
                    .on_click_stop(move |_| {
                        if reference > 0 {
                            let dap = local_terminal.get_active_dap(false);
                            if let Some(dap) = dap {
                                let process_stopped = local_terminal
                                    .get_terminal(&dap.term_id)
                                    .and_then(|t| {
                                        t.run_debug
                                            .with(|r| r.as_ref().map(|r| r.stopped))
                                    })
                                    .unwrap_or(
                                        local_terminal
                                            .debug
                                            .active_dap
                                            .get_untracked()
                                            != Some(dap.dap_id),
                                    );
                                if !process_stopped {
                                    dap.toggle_expand(
                                        node.parent.clone(),
                                        reference,
                                    );
                                }
                            }
                        }
                    })
                    .style(move |s| {
                        s.items_center()
                            .padding_right(10.0)
                            .padding_left((level * 10) as f32)
                            .min_width_pct(100.0)
                            .hover(|s| {
                                s.apply_if(reference > 0, |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                })
                            })
                    })
                },
            )
            .item_size_fixed(move || ui_line_height.get())
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| s.absolute().size_full()),
    )
    .style(|s| {
        s.width_full()
            .line_height(1.6)
            .flex_grow(1.0f32)
            .flex_basis(0)
    })
}

pub(super) fn breakpoints_view(window_tab_data: Rc<WindowTabData>) -> impl View {
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
                            .style(|s| {
                                s.margin_right(6.0).cursor(CursorStyle::Pointer)
                            })
                            .on_click_stop(move |_| {
                                breakpoints.update(|breakpoints| {
                                    if let Some(breakpoints) =
                                        breakpoints.get_mut(&full_path)
                                    {
                                        if let Some(breakpoint) =
                                            breakpoints.get_mut(&line)
                                        {
                                            breakpoint.active = !breakpoint.active;
                                        }
                                    }
                                });
                            }),
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
