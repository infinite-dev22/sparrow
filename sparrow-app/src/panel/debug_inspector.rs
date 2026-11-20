//! Compact frame sidebar and evaluation/variables workspace.
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    panel::position::PanelPosition,
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View, ViewId,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    style::CursorStyle,
    views::{
        Decorators, TooltipExt, container, dyn_container, dyn_stack, empty, label,
        scroll, stack, svg,
    },
};
use std::rc::Rc;

fn placeholder(message: &'static str, window: &WindowTabData) -> impl View + use<> {
    let config = window.common.config;
    container(label(move || message)).style(move |s| {
        s.size_full()
            .items_center()
            .justify_center()
            .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
    })
}

pub(super) fn inspector(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window.common.config;
    let show_breakpoints = create_rw_signal(false);
    let total = create_rw_signal(800.0_f64);
    let desired = create_rw_signal(370.0_f64);
    let width = move || desired.get().min((total.get() - 120.0).max(80.0));
    let left = stack((
        frames(window.clone())
            .style(|s| s.flex_grow(1.0_f32).flex_basis(0).min_height(0.0)),
        label(move || {
            if show_breakpoints.get() {
                "‹ Threads & Variables"
            } else {
                "Breakpoints…"
            }
        })
        .on_click_stop(move |_| show_breakpoints.update(|show| *show = !*show))
        .style(move |s| {
            s.height(24.0)
                .padding_horiz(8.0)
                .items_center()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .cursor(CursorStyle::Pointer)
        }),
    ))
    .style(move |s| {
        s.flex_col()
            .width(width())
            .min_width(0.0)
            .height_full()
            .flex_shrink(0.0_f32)
    });
    let divider = empty();
    let divider_id = divider.id();
    let drag = create_rw_signal(None);
    let divider = divider
        .on_event_stop(EventListener::PointerDown, move |event| {
            if let Event::PointerDown(event) = event {
                drag.set(Some(event.pos.x));
                divider_id.request_active();
            }
        })
        .on_event_stop(EventListener::PointerMove, move |event| {
            if let Event::PointerMove(event) = event {
                if let Some(start) = drag.get_untracked() {
                    desired.set(
                        (width() + event.pos.x - start)
                            .clamp(80.0, (total.get_untracked() - 120.0).max(80.0)),
                    );
                }
            }
        })
        .on_event_stop(EventListener::PointerUp, move |_| drag.set(None))
        .style(move |s| {
            s.width(1.0)
                .height_full()
                .flex_shrink(0.0_f32)
                .background(config.get().color(SparrowColor::SPARROW_BORDER))
                .cursor(CursorStyle::ColResize)
        });
    let right_window = window.clone();
    let right = dyn_container(
        move || show_breakpoints.get(),
        move |show| {
            if show {
                super::debug_view::breakpoints_view(right_window.clone()).into_any()
            } else {
                variables_workspace(right_window.clone()).into_any()
            }
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0)
            .min_width(0.0)
            .height_full()
    });
    stack((left, divider, right))
        .style(|s| s.size_full().min_width(0.0))
        .on_resize(move |rect| total.set(rect.width()))
}

fn frames(window: Rc<WindowTabData>) -> impl View {
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
                return placeholder("Frames are not available", &frame_window)
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
            let dropdown = crate::dropdown::dropdown(thread, threads, config)
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
            let list = scroll(
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
                            s.height(22.0)
                                .width_full()
                                .min_width(0.0)
                                .items_center()
                                .padding_horiz(8.0)
                                .cursor(CursorStyle::Pointer)
                                .apply_if(
                                    selected.get().map(|f| f.id).unwrap_or_else(
                                        || {
                                            traces
                                                .with(|t| {
                                                    t.get(&main).and_then(|t| {
                                                        t.frames.with(|f| {
                                                            f.front().map(|f| f.id)
                                                        })
                                                    })
                                                })
                                                .unwrap_or(usize::MAX)
                                        },
                                    ) == frame_id,
                                    |s| {
                                        s.background(config.get().color(
                                            SparrowColor::PANEL_SELECTION_BACKGROUND,
                                        ))
                                    },
                                )
                                .hover(|s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                })
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

// Intercept submission before the stock TextInput consumes Enter. All other
// events still use the shared input's editing, focus, IME and clipboard behavior.
struct ExpressionInput {
    input: floem::views::TextInput,
    submit: Rc<dyn Fn(bool)>,
}
impl View for ExpressionInput {
    fn id(&self) -> ViewId {
        self.input.id()
    }
    fn update(
        &mut self,
        cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) {
        self.input.update(cx, state);
    }
    fn style_pass(&mut self, cx: &mut floem::context::StyleCx) {
        self.input.style_pass(cx);
    }
    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::tree::NodeId {
        self.input.layout(cx)
    }
    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<floem::kurbo::Rect> {
        self.input.compute_layout(cx)
    }
    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        self.input.paint(cx);
    }
    fn event_before_children(
        &mut self,
        cx: &mut floem::context::EventCx,
        event: &Event,
    ) -> EventPropagation {
        if let Event::KeyDown(key) = event {
            if key.key.logical_key == Key::Named(NamedKey::Enter) {
                (self.submit)(key.modifiers.control() && key.modifiers.shift());
                return EventPropagation::Stop;
            }
        }
        self.input.event_before_children(cx, event)
    }
}

fn variables_workspace(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let input = create_rw_signal(String::new());
    let terminal = window.terminal.clone();
    let submit = Rc::new(move |watch| {
        if let Some(dap) = terminal.get_active_dap(false) {
            dap.evaluate(&input.get_untracked(), watch);
        }
    });
    let field = ExpressionInput {
        input: crate::text_input::text_input(input, config).placeholder(
            "Evaluate expression (Enter) or add a watch (Ctrl+Shift+Enter)",
        ),
        submit: submit.clone(),
    }
    .style(|s| s.flex_grow(1.0_f32).flex_basis(0).min_width(0.0));
    let input_row = stack((
        field,
        clickable_icon(
            || SparrowIcons::ADD,
            move || submit(true),
            || false,
            || false,
            || "Add watch (Ctrl+Shift+Enter)",
            config,
        ),
    ))
    .style(|s| {
        s.width_full()
            .min_width(0.0)
            .items_center()
            .flex_shrink(0.0_f32)
    });
    let results_terminal = window.terminal.clone();
    let results = dyn_stack(
        move || {
            results_terminal
                .get_active_dap(true)
                .map(|dap| {
                    dap.stopped.get();
                    dap.evaluation
                        .entries
                        .get()
                        .into_iter()
                        .map(|entry| (dap.clone(), entry))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        },
        |(dap, entry)| {
            (
                dap.dap_id,
                entry.request_id,
                entry.result.clone(),
                dap.stopped.get_untracked(),
            )
        },
        move |(dap, entry)| {
            let request_id = entry.request_id;
            let message = match &entry.result {
                Some(Ok(result)) => format!("{} = {result}", entry.expression),
                Some(Err(error)) => format!("{}: {error}", entry.expression),
                None => format!(
                    "{}: {}",
                    entry.expression,
                    if dap.stopped.get_untracked() {
                        "Evaluating…"
                    } else {
                        "Not available while running"
                    }
                ),
            };
            let full_message = message.clone();
            let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
            let error = matches!(entry.result, Some(Err(_)));
            stack((
                label(move || message.clone())
                    .style(move |s| {
                        s.min_width(0.0)
                            .flex_grow(1.0_f32)
                            .text_ellipsis()
                            .apply_if(error, |s| {
                                s.color(config.get().color(
                                    SparrowColor::ERROR_LENS_ERROR_FOREGROUND,
                                ))
                            })
                    })
                    .tooltip(move || {
                        let message = full_message.clone();
                        label(move || message.clone()).style(|s| s.max_width(700.0))
                    })
                    .style(|s| s.flex_grow(1.0_f32).flex_basis(0).min_width(0.0)),
                clickable_icon(
                    || SparrowIcons::CLOSE,
                    move || {
                        dap.evaluation.entries.update(|entries| {
                            entries.retain(|entry| entry.request_id != request_id)
                        })
                    },
                    || false,
                    || false,
                    || "Remove expression",
                    config,
                ),
            ))
            .style(|s| {
                s.height(24.0)
                    .items_center()
                    .padding_left(8.0)
                    .min_width(0.0)
                    .width_full()
            })
        },
    )
    .style(|s| s.flex_col().width_full().max_height(150.0));
    let result_height = window.terminal.clone();
    let results = scroll(results).style(move |s| {
        let count = result_height
            .get_active_dap(true)
            .map(|dap| dap.evaluation.entries.with(|entries| entries.len()))
            .unwrap_or(0);
        s.width_full()
            .height((count as f32 * 24.0).min(144.0))
            .flex_shrink(0.0_f32)
    });
    let body_window = window.clone();
    let body_terminal = window.terminal.clone();
    let body = dyn_container(
        move || {
            body_terminal.get_active_dap(true).is_some_and(|dap| {
                dap.stopped.get() && dap.variables.with(|v| !v.children.is_empty())
            })
        },
        move |available| {
            if available {
                super::debug_view::variables_view(body_window.clone()).into_any()
            } else {
                placeholder("Variables are not available", &body_window).into_any()
            }
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0)
            .min_height(0.0)
            .width_full()
    });
    stack((input_row, results, body))
        .style(|s| s.flex_col().size_full().min_width(0.0))
}
