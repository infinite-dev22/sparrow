//! Status bar summary and animation clock for active tasks.

use crate::window_tab::WindowTabData;
use floem::{
    View,
    reactive::{Memo, RwSignal, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, label, stack},
};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use super::{Activity, activity::activities};
use crate::widgets::progress_bar;

pub fn status(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let tasks = window.common.background_tasks.clone();
    let open = tasks.open;
    let items = activities(window);
    let phase = tasks.phase;
    let generation = Rc::new(Cell::new(0));
    floem::reactive::create_effect(move |previous: Option<bool>| {
        let active = !items.get().is_empty();
        if previous != Some(active) {
            generation.set(generation.get() + 1);
            if active {
                animate(
                    phase,
                    items,
                    generation.clone(),
                    generation.get(),
                    Instant::now(),
                );
            }
        }
        if !active {
            open.set(false);
        }
        active
    });
    stack((
        label(move || {
            items.with(|items| {
                items
                    .first()
                    .map(|item| {
                        let title = if item.message.is_empty() {
                            item.title.clone()
                        } else {
                            format!("{}: {}", item.title, item.message)
                        };
                        if items.len() > 1 {
                            format!("{title} (+{})", items.len() - 1)
                        } else {
                            title
                        }
                    })
                    .unwrap_or_default()
            })
        })
        .style(|s| s.min_width(0.0).max_width(260.0).text_ellipsis()),
        progress_bar(
            move || {
                items.with(|items| items.first().and_then(|item| item.percentage))
            },
            phase,
            config,
        )
        .style(|s| s.width(100.0).height(4.0).flex_shrink(0.0_f32)),
        label(move || {
            items.with(|items| {
                items
                    .first()
                    .and_then(|item| item.percentage)
                    .map(|percent| format!("{percent}%"))
                    .unwrap_or_default()
            })
        })
        .style(move |s| {
            s.min_width(30.0).flex_shrink(0.0_f32).apply_if(
                items.with(|items| {
                    items.first().and_then(|item| item.percentage).is_none()
                }),
                |s| s.hide(),
            )
        }),
        label(|| "⌃"),
    ))
    .on_click_stop(move |_| open.update(|value| *value = !*value))
    .style(move |s| {
        s.height_full()
            .min_width(0.0)
            .max_width(390.0)
            .items_center()
            .gap(8.0)
            .padding_horiz(10.0)
            .cursor(floem::style::CursorStyle::Pointer)
            .apply_if(items.with(|items| items.is_empty()), |s| s.hide())
    })
}

fn animate(
    phase: RwSignal<f64>,
    items: Memo<Vec<Activity>>,
    generation: Rc<Cell<u64>>,
    ticket: u64,
    started: Instant,
) {
    floem::action::exec_after(Duration::from_millis(16), move |_| {
        if generation.get() == ticket
            && items
                .try_get_untracked()
                .is_some_and(|items| !items.is_empty())
        {
            phase.set(started.elapsed().as_secs_f64());
            animate(phase, items, generation, ticket, started);
        }
    });
}
