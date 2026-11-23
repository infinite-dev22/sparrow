//! Before-launch task list and task selection controls.

use super::Dialog;
use crate::config::color::SparrowColor;
use crate::dart::run_config::{self, TaskDraft};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{KeyCode, PhysicalKey},
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, label, stack},
};
use std::rc::Rc;

pub(super) fn task_list(d: Rc<Dialog>) -> impl View {
    let config = d.window.common.config;
    let keyboard = d.keyboard.clone();
    let selected_task = d.task_selected;
    let before = d.fields[6];
    let before_args = d.fields[7];
    dyn_stack(
        {
            let d = d.clone();
            move || {
                d.current_tracked()
                    .with(|drafts| {
                        drafts
                            .get(d.selected.get())
                            .map(|draft| draft.tasks.clone())
                            .unwrap_or_default()
                    })
                    .into_iter()
                    .enumerate()
                    .collect::<Vec<_>>()
            }
        },
        |(index, _)| *index,
        {
            let d = d.clone();
            let keyboard = keyboard.clone();
            move |(index, _task): (usize, TaskDraft)| {
                let select = d.clone();
                let toggle = d.clone();
                let enabled_state = d.clone();
                let program_state = d.clone();
                let checked = move || {
                    enabled_state.current_tracked().with(|drafts| {
                        drafts
                            .get(enabled_state.selected.get())
                            .and_then(|draft| draft.tasks.get(index))
                            .is_some_and(|task| task.enabled)
                    })
                };
                let set_enabled = move |enabled| {
                    toggle.capture();
                    toggle.current_tracked().update(|drafts| {
                        if let Some(task) = drafts
                            .get_mut(toggle.selected.get_untracked())
                            .and_then(|draft| draft.tasks.get_mut(index))
                        {
                            task.enabled = enabled;
                        }
                    });
                };
                let activate = set_enabled.clone();
                let is_enabled = checked.clone();
                keyboard.control(
                    stack((
                        keyboard.control(
                            crate::widgets::checkbox(checked, config)
                                .on_update(set_enabled)
                                // ModalFocus moves focus to the wrapper. Pointer clicks
                                // still use the native checkbox's on_update handler.
                                .on_event(EventListener::KeyDown, move |event| {
                                    if let Event::KeyDown(key) = event {
                                        if matches!(
                                            key.key.physical_key,
                                            PhysicalKey::Code(
                                                KeyCode::Space
                                                    | KeyCode::Enter
                                                    | KeyCode::NumpadEnter
                                            )
                                        ) {
                                            activate(!is_enabled());
                                            return EventPropagation::Stop;
                                        }
                                    }
                                    EventPropagation::Continue
                                }),
                        ),
                        label(move || {
                            if selected_task.get() == index {
                                format!(
                                    "{}. {} {}",
                                    index + 1,
                                    run_config::task_title(
                                        &before.get(),
                                        &before_args.get()
                                    ),
                                    ""
                                )
                            } else {
                                format!(
                                    "{}. {}",
                                    index + 1,
                                    program_state.current_tracked().with(|drafts| {
                                        drafts
                                            .get(program_state.selected.get())
                                            .and_then(|draft| draft.tasks.get(index))
                                            .map(|task| {
                                                run_config::task_title(
                                                    &task.program,
                                                    &task.args,
                                                )
                                            })
                                            .unwrap_or_default()
                                    })
                                )
                            }
                        })
                        .style(|s| {
                            s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()
                        }),
                    ))
                    .on_click_stop(move |_| select.choose_task(index))
                    .style(move |s| {
                        s.height(24.0)
                            .width_full()
                            .items_center()
                            .gap(6.0)
                            .padding_horiz(8.0)
                            .apply_if(selected_task.get() == index, |s| {
                                s.background(config.get().color(
                                    SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                ))
                            })
                    }),
                )
            }
        },
    )
    .style(|s| s.flex_col().width_full())
}
