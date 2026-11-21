//! Background task detail list and cancellation controls.

use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo},
    views::{Decorators, dyn_stack, empty, label, scroll, stack},
};
use std::rc::Rc;

use super::activity::activities;
use crate::widgets::progress_bar;

pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let tasks = window.common.background_tasks.clone();
    let open = tasks.open;
    let items = activities(window);
    // One shared frame clock drives all visible bars and stops when idle.
    let view = stack((
        empty()
            .style(|s| s.absolute().size_full())
            .on_click_stop(move |_| open.set(false)),
        stack((
            stack((
                label(move || {
                    format!("Background Tasks ({})", items.with(|items| items.len()))
                })
                .style(|s| s.flex_grow(1.0_f32).font_bold()),
                label(|| "×").on_click_stop(move |_| open.set(false)),
            ))
            .style(|s| s.width_full().items_center()),
            scroll(
                dyn_stack(
                    move || items.get(),
                    |item| item.id.clone(),
                    move |item| {
                        let cancel_tasks = tasks.clone();
                        let id = item.id.clone();
                        let row_id = item.id.clone();
                        let current = create_memo(move |_| items.with(|items| items.iter().find(|a| a.id == row_id).cloned()));
                        let phase = tasks.phase;
                        stack((
                            stack((
                                label(move || current.get().map(|a| a.title).unwrap_or_default()).style(|s| {
                                    s.font_bold().min_width(0.0).text_ellipsis()
                                }),
                                label(move || current.get().map(|a| {
                                    if a.cancelling { "Cancelling…".into() }
                                    else if let Some(percent) = a.percentage { format!("{} · {percent}%", a.message) }
                                    else if a.message.is_empty() { "In progress…".into() }
                                    else { a.message }
                                }).unwrap_or_default()).style(move |s| {
                                    s.min_width(0.0).text_ellipsis().color(
                                        config.get().color(
                                            SparrowColor::PANEL_FOREGROUND_DIM,
                                        ),
                                    )
                                }),
                                progress_bar(move || current.get().and_then(|a| a.percentage), phase, config)
                                    .style(|s| s.width_full().height(4.0)),
                            ))
                            .style(|s| {
                                s.flex_col()
                                    .flex_grow(1.0_f32)
                                    .min_width(0.0)
                                    .gap(4.0)
                            }),
                            label(|| "Cancel")
                                .on_click_stop(move |_| cancel_tasks.cancel(&id))
                                .style(move |s| {
                                    s.color(
                                        config
                                            .get()
                                            .color(SparrowColor::EDITOR_LINK),
                                    )
                                    .apply_if(!current.get().is_some_and(|a| a.cancellable && !a.cancelling), |s| s.hide())
                                }),
                        ))
                        .style(|s| {
                            s.width_full().items_center().gap(12.0).padding_vert(8.0)
                        })
                    },
                )
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.width_full().max_height(300.0)),
        ))
        .on_event_stop(EventListener::PointerDown, |_| {})
        .on_click_stop(|_| {})
        .style(move |s| {
            s.absolute()
                .inset_right(12.0)
                .inset_bottom(config.get().ui.status_height() as f64 + 6.0)
                .width(420.0)
                .max_width_pct(95.0)
                .padding(14.0)
                .gap(8.0)
                .flex_col()
                .border(1.0)
                .border_radius(6.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .z_index(190)
            .apply_if(!open.get() || items.with(|items| items.is_empty()), |s| {
                s.hide()
            })
    })
    .keyboard_navigable()
    .on_event(EventListener::KeyDown, move |event| {
        if matches!(event, Event::KeyDown(key) if key.key.logical_key == Key::Named(NamedKey::Escape)) {
            open.set(false);
            EventPropagation::Stop
        } else { EventPropagation::Continue }
    });
    let id = view.id();
    floem::reactive::create_effect(move |_| {
        if open.get() {
            id.request_focus();
        }
    });
    view
}
