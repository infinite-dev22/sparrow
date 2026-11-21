//! File refactoring review dialog and action controls.

use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, empty, label, scroll, stack},
};
use std::rc::Rc;

pub fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.file_refactor.clone();
    let config = window.common.config;
    let active = state.active;
    crate::widgets::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let name = state.name;
    let busy = state.busy;
    let comments = state.comments;
    let moving = state.moving;
    let deleting = state.deleting;
    let scope = state.scope;
    let plan = state.plan;
    let error = state.error;
    let preview = state.clone();
    let apply = state.clone();
    let cancel = state.clone();
    let w1 = window.clone();
    let w2 = window.clone();
    let w3 = window.clone();
    let close = state.clone();
    let w4 = window.clone();
    let focus = crate::widgets::modal_focus::ModalFocus::default();
    let enter = state.clone();
    let ew = window.clone();
    focus.set_submit(move || enter.run(ew.clone(), false));
    let input = focus
        .input(name, config)
        .disabled(move || busy.get() || deleting.get());
    let input_id = input.id();
    let initial_focus = focus.clone();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            floem::action::exec_after(std::time::Duration::from_millis(30), {
                let focus = initial_focus.clone();
                move |_| {
                    if deleting.get_untracked() {
                        focus.focus_first();
                    } else {
                        input_id.request_focus();
                    }
                }
            });
        }
    });
    floem::reactive::create_effect(move |_| {
        name.get();
        plan.set(None);
    });
    let refactor_button = focus.control(
        crate::widgets::button(
            move || if deleting.get() { "Delete" } else { "Refactor" },
            crate::widgets::ButtonKind::Primary,
            config,
        )
        .on_click_stop(move |_| apply.run(w1.clone(), false))
        .disabled(move || busy.get())
        .style(|s| s.padding_horiz(12.0).height(26.0).border_radius(3.0).border(0.0)),
    );
    let preview_button = focus.control(
        crate::widgets::button(|| "Preview", crate::widgets::ButtonKind::Plain, config)
            .on_click_stop(move |_| preview.run(w2.clone(), true))
            .disabled(move || busy.get())
            .style(|s| s.padding_horiz(8.0).height(26.0)),
    );
    let cancel_button = focus.control(
        crate::widgets::button(|| "Cancel", crate::widgets::ButtonKind::Plain, config)
            .on_click_stop(move |_| cancel.close(&w3))
            .disabled(move || busy.get())
            .style(|s| s.padding_horiz(8.0).height(26.0)),
    );
    let body = stack((
        stack((
            label(move || {
                if deleting.get() {
                    "Safe Delete"
                } else if moving.get() {
                    "Move File"
                } else {
                    "Rename"
                }
            })
            .style(|s| s.flex_grow(1.0_f32).font_bold()),
            label(|| "×").on_click_stop(move |_| close.close(&w4)),
        ))
        .style(|s| s.height(34.0).items_center().padding_horiz(12.0)),
        stack((
            label(move || {
                let path = state.path.get();
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if deleting.get() {
                    format!("Check project usages before deleting '{name}':")
                } else {
                    format!(
                        "{} file '{name}' and its usages to:",
                        if moving.get() { "Move" } else { "Rename" }
                    )
                }
            }),
            input,
            crate::widgets::labeled_checkbox(
                move || comments.get(),
                || "Search in comments and strings",
                move |checked| {
                    comments.set(checked);
                    plan.set(None);
                },
                config,
            ),
            label(|| "Scope:"),
            label(move || {
                if scope.get() {
                    "Directory"
                } else {
                    "Project Files"
                }
            })
            .ide_popout_menu(move || {
                crate::widgets::popup_menu::Menu::new("")
                    .entry(crate::widgets::popup_menu::MenuItem::new("Project Files").action(
                        move || {
                            scope.set(false);
                            plan.set(None);
                        },
                    ))
                    .entry(crate::widgets::popup_menu::MenuItem::new("Directory").action(
                        move || {
                            scope.set(true);
                            plan.set(None);
                        },
                    ))
            })
            .style(move |s| {
                s.width_full()
                    .height(26.0)
                    .items_center()
                    .padding_horiz(6.0)
                    .border(1.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            }),
            scroll(label(move || {
                plan.with(|p| p.as_ref().map(|p| p.summary()).unwrap_or_default())
            }))
            .style(move |s| {
                s.max_height(230.0)
                    .width_full()
                    .apply_if(plan.with(|p| p.is_none()), |s| s.hide())
            }),
            label(move || {
                if busy.get() {
                    "Searching usages…".into()
                } else {
                    error.get()
                }
            })
            .style(move |s| {
                s.width_full()
                    .apply_if(!busy.get() && error.with(|s| s.is_empty()), |s| {
                        s.hide()
                    })
            }),
            stack((
                empty().style(|s| s.flex_grow(1.0_f32)),
                refactor_button,
                preview_button,
                cancel_button,
            ))
            .style(|s| s.gap(10.0).width_full()),
        ))
        .style(|s| s.flex_col().width_full().padding(12.0).gap(12.0)),
    ))
    .style(move |s| {
        s.width(420.0)
            .max_width_pct(95.0)
            .flex_col()
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
            .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
    });
    let dialog = stack((
        container(body).style(|s| s.size_full().items_center().justify_center()),
    ))
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key) = event {
            if key.key.logical_key == Key::Named(NamedKey::Escape) {
                state.close(&window);
                return EventPropagation::Stop;
            }
        }
        EventPropagation::Continue
    })
    .style(move |s| {
        s.absolute()
            .size_full()
            .z_index(100)
            .apply_if(!active.get(), |s| s.hide())
    });
    focus.bind_root(dialog, active, config)
}
use crate::widgets::popup_menu::PopupMenuExt;
