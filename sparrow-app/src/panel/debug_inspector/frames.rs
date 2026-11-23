//! Debugger thread selector and stack frame list.

use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    views::{
        Decorators, dyn_container, dyn_stack, empty, label, stack, svg,
    },
};
use std::rc::Rc;
use crate::widgets::sparrow_scroll;

pub(super) fn frames(window: Rc<WindowTabData>) -> impl View {
    let terminal = window.terminal.clone();
    let config = window.common.config;
    let frame_window = window.clone();
    dyn_container(
        move || {
            terminal.get_active_dap(true).map(|dap| {
                (dap.dap_id, dap.stopped.get(), dap.pause_generation.get())
            })
        },
        move |state| {
            let Some((id, true, _)) = state else {
                return crate::widgets::empty_state(|| "Frames are not available", config)
                    .into_any();
            };
            let Some(dap) = frame_window
                .terminal
                .debug
                .daps
                .get_untracked()
                .get(&id)
                .cloned()
            else {
                return empty().into_any();
            };
            let main = dap.thread_id.get_untracked().unwrap_or_default();
            let thread = create_rw_signal(main);
            let threads = dap
                .stack_traces
                .get_untracked()
                .keys()
                .copied()
                .collect::<Vec<_>>();
            let dropdown = crate::widgets::dropdown(thread, threads, config)
                .main_view(move |id| {
                    stack((
                        label(move || {
                            if id == main {
                                "✓ main".to_owned()
                            } else {
                                format!("Thread {id}")
                            }
                        })
                        .style(|s| {
                            s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()
                        }),
                        svg(move || config.get().ui_svg(SparrowIcons::PALETTE_MENU))
                            .style(move |s| {
                                s.size(12.0, 12.0).color(
                                    config
                                        .get()
                                        .color(SparrowColor::SPARROW_ICON_ACTIVE),
                                )
                            }),
                    ))
                    .style(|s| s.items_center().size_full())
                    .into_any()
                })
                .list_item_view(move |id| {
                    label(move || {
                        if id == main {
                            "main".to_owned()
                        } else {
                            format!("Thread {id}")
                        }
                    })
                    .style(|s| s.height(28.0).padding_horiz(6.0).items_center())
                    .into_any()
                })
                .on_accept({
                    let terminal = frame_window.terminal.clone();
                    let dap = dap.clone();
                    move |id| {
                        thread.set(id);
                        if let Some(frame) =
                            dap.stack_traces.get_untracked().get(&id).and_then(
                                |trace| {
                                    trace.frames.get_untracked().front().cloned()
                                },
                            )
                        {
                            terminal.dap_frame_scopes(dap.dap_id, frame.id);
                        }
                    }
                })
                .style(|s| {
                    s.width_full()
                        .border(0.0)
                        .border_radius(0.0)
                        .flex_shrink(0.0_f32)
                });
            let selected = dap.selected_frame;
            let traces = dap.stack_traces;
            let terminal = frame_window.terminal.clone();
            let internal = frame_window.common.internal_command;
            let list = sparrow_scroll(
                dyn_stack(
                    move || {
                        traces.with(|traces| {
                            traces
                                .get(&thread.get())
                                .map(|trace| trace.frames.get())
                                .unwrap_or_default()
                        })
                    },
                    |frame| frame.id,
                    move |frame| {
                        let frame_id = frame.id;
                        let source =
                            frame.source.as_ref().and_then(|s| s.path.clone());
                        let description = source
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map(|p| {
                                format!("{}:{}", p.to_string_lossy(), frame.line)
                            })
                            .unwrap_or_default();
                        let name = format!("{} ({description})", frame.name);
                        let terminal = terminal.clone();
                        stack((
                            svg(move || config.get().ui_svg(SparrowIcons::FILE))
                                .style(move |s| {
                                    s.size(14.0, 14.0).margin_right(5.0).color(
                                        config.get().color(
                                            SparrowColor::SPARROW_ICON_ACTIVE,
                                        ),
                                    )
                                }),
                            label(move || name.clone())
                                .style(|s| s.min_width(0.0).text_ellipsis()),
                        ))
                        .on_click_stop(move |_| {
                            terminal.dap_frame_scopes(id, frame_id);
                            if let Some(path) = source.clone() {
                                internal.send(
                                    InternalCommand::DebugJumpToLocation {
                                        location: EditorLocation {
                                            path,
                                            position: Some(
                                                EditorPosition::Position(
                                                    lsp_types::Position::new(
                                                        frame.line.saturating_sub(1)
                                                            as u32,
                                                        frame
                                                            .column
                                                            .saturating_sub(1)
                                                            as u32,
                                                    ),
                                                ),
                                            ),
                                            scroll_offset: None,
                                            ignore_unconfirmed: false,
                                            same_editor_tab: false,
                                        },
                                    },
                                );
                            }
                        })
                        .style(move |s| {
                            let selected_id =
                                selected.get().map(|frame| frame.id).or_else(|| {
                                    traces.with(|traces| {
                                        traces.get(&main).and_then(|trace| {
                                            trace.frames.with(|frames| {
                                                frames.front().map(|frame| frame.id)
                                            })
                                        })
                                    })
                                });
                            crate::widgets::selectable_row_style(
                                s.height(22.0)
                                    .width_full()
                                    .min_width(0.0)
                                    .items_center()
                                    .padding_horiz(8.0),
                                &config.get(),
                                selected_id == Some(frame_id),
                            )
                        })
                    },
                )
                .style(|s| s.flex_col().width_full().min_width(0.0)),
            )
            .style(|s| {
                s.flex_grow(1.0_f32)
                    .flex_basis(0)
                    .min_height(0.0)
                    .width_full()
            });
            stack((dropdown, list))
                .style(|s| s.flex_col().size_full())
                .into_any()
        },
    )
    .style(|s| s.size_full())
}
