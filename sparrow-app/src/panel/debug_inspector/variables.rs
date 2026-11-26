//! Debug variables, watches and expression evaluation.

use crate::widgets::tooltip::TooltipExt;
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    views::{Decorators, dyn_container, dyn_stack, label, stack},
};
use std::rc::Rc;
use crate::widgets::sparrow_scroll;

pub(super) fn variables_workspace(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let input = create_rw_signal(String::new());
    let terminal = window.terminal.clone();
    let submit = Rc::new(move |watch| {
        if let Some(dap) = terminal.get_active_dap(false) {
            dap.evaluate(&input.get_untracked(), watch);
        }
    });
    let key_submit = submit.clone();
    let field = crate::widgets::TextField::new(input)
        .colors(move || crate::widgets::m3_from_config(config))
        .placeholder(
            "Evaluate expression (Enter) or add a watch (Ctrl+Shift+Enter)",
        )
        .on_input_event(EventListener::KeyUp, move |event| {
            if let Event::KeyUp(key) = event {
                if key.key.logical_key == Key::Named(NamedKey::Enter) {
                    key_submit(key.modifiers.control() && key.modifiers.shift());
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
        .into_view()
        .style(|s| {
            s.width(floem::unit::PxPctAuto::Auto)
                .flex_grow(1.0_f32)
                .flex_basis(0)
                .min_width(0.0)
        });
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
    let results = sparrow_scroll(results).style(move |s| {
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
                crate::panel::debug_view::variables_view(body_window.clone()).into_any()
            } else {
                crate::widgets::empty_state(|| "Variables are not available", config).into_any()
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
