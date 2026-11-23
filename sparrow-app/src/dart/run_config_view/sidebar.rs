//! Run configuration sidebar, template selection and list shortcuts.

use crate::config::{color::SparrowColor, icon::SparrowIcons};
use crate::dart::run_config::{self, Draft};
use crate::widgets::modal_focus::{ModalChoice, list_destination};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, Modifiers, NamedKey},
    reactive::{Scope, SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{Decorators, dyn_stack, label, stack, svg},
};
use std::rc::Rc;

use super::Dialog;
use floem::reactive::Memo;
use crate::widgets::sparrow_scroll;

pub(super) fn configuration_sidebar(
    d: Rc<Dialog>,
    has_selected: Memo<bool>,
    cx: Scope,
) -> impl View {
    let config = d.window.common.config;
    let keyboard = d.keyboard.clone();
    let templates_mode = d.templates_mode;
    let add = d.clone();
    let remove = d.clone();
    let duplicate = d.clone();
    let list_focus = cx.create_rw_signal(None::<floem::ViewId>);
    let configuration_list = sparrow_scroll(
        dyn_stack(
            {
                let d = d.clone();
                move || d.drafts.get().into_iter().enumerate().collect::<Vec<_>>()
            },
            |(i, draft)| (*i, draft.flutter()),
            {
                let d = d.clone();
                move |(i, draft)| {
                    let select = d.clone();
                    let active = d.selected;
                    let name = d.fields[0];
                    let drafts = d.drafts;
                    stack((
                        svg({
                            let flutter = draft.flutter();
                            move || {
                                config.get().ui_svg(if flutter {
                                    SparrowIcons::FLUTTER
                                } else {
                                    SparrowIcons::DART
                                })
                            }
                        })
                        .style(|s| s.size(16.0, 16.0)),
                        label(move || {
                            if !templates_mode.get() && active.get() == i {
                                name.get()
                            } else {
                                drafts.with(|drafts| {
                                    drafts
                                        .get(i)
                                        .map(|draft| draft.name.clone())
                                        .unwrap_or_default()
                                })
                            }
                        }),
                    ))
                    .on_click_stop(move |_| {
                        select.choose_config(i);
                        if let Some(id) = list_focus.get_untracked() {
                            id.request_focus();
                        }
                    })
                    .style(move |s| {
                        s.height(22.0)
                            .width_full()
                            .padding_left(36.0)
                            .gap(4.0)
                            .items_center()
                            .border_radius(4.0)
                            .apply_if(
                                !templates_mode.get() && active.get() == i,
                                |s| {
                                    s.background(config.get().color(
                                        SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                    ))
                                },
                            )
                            .hover(|s| s.cursor(CursorStyle::Pointer))
                    })
                }
            },
        )
        .style(|s| s.flex_col().min_width_full()),
    )
    .style(|s| s.flex_grow(1.0_f32).min_height(0.0));
    let selected_row = d.selected;
    let configuration_list = configuration_list.ensure_visible(move || {
        let y = selected_row.get() as f64 * 22.0;
        floem::kurbo::Rect::new(0.0, y, 1.0, y + 22.0)
    });
    let list_id = configuration_list.id();
    list_focus.set(Some(list_id));
    let list_dialog = d.clone();
    let configuration_list = keyboard
        .control_if(configuration_list, move || has_selected.get_untracked())
        .style(|s| s.focus(|s| s.background(floem::peniko::Color::TRANSPARENT)))
        .on_event(EventListener::KeyDown, move |event| {
            let Event::KeyDown(key) = event else {
                return EventPropagation::Continue;
            };
            if key.key.logical_key == Key::Named(NamedKey::Delete) && key.modifiers.is_empty() {
                list_dialog.remove(); return EventPropagation::Stop;
            }
            if key.modifiers == Modifiers::CONTROL && matches!(&key.key.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("d")) {
                list_dialog.duplicate(); return EventPropagation::Stop;
            }
            if !key.modifiers.is_empty() { return EventPropagation::Continue; }
            let len = list_dialog.current_tracked().with_untracked(Vec::len);
            if let Some(index) = list_destination(
                &key.key.logical_key,
                list_dialog.selected.get_untracked(),
                len,
            ) {
                list_dialog.choose(index);
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        });
    let template_link = d.clone();
    let d_selected = d.selected;
    let template_rows =
        (0..run_config::TEMPLATE_NAMES.len()).map(|index| {
            let choose = d.clone();
            keyboard.control(
                label(move || format!("    {}", run_config::TEMPLATE_NAMES[index]))
                    .on_click_stop(move |_| choose.choose_template(index))
                    .style(move |s| {
                        s.height(24.0).width_full().items_center().apply_if(
                            templates_mode.get() && d_selected.get() == index,
                            |s| {
                                s.background(config.get().color(
                                    SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                ))
                            },
                        )
                    }),
            )
        });
    stack((
        stack((
            keyboard.menu(
                svg(move || config.get().ui_svg(SparrowIcons::ADD)).style(|s| {
                    s.size(16.0, 16.0)
                        .margin_horiz(5.0)
                        .cursor(CursorStyle::Pointer)
                }),
                move || {
                    run_config::TEMPLATE_NAMES
                        .iter()
                        .enumerate()
                        .map(|(i, name)| {
                            let add = add.clone();
                            ModalChoice::new(*name, move || add.add(i))
                        })
                        .collect()
                },
            ),
            keyboard.icon(
                || SparrowIcons::WINDOW_MINIMIZE,
                move || remove.remove(),
                || false,
                {
                    let drafts = d.drafts;
                    move || templates_mode.get() || drafts.with(Vec::is_empty)
                },
                || "Remove Configuration (Delete)",
                config,
            ),
            keyboard.icon(
                || SparrowIcons::FILE,
                move || duplicate.duplicate(),
                || false,
                move || templates_mode.get() || !has_selected.get(),
                || "Duplicate Configuration (Ctrl+D)",
                config,
            ),
        ))
        .style(|s| s.height(26.0).items_center()),
        label({
            let drafts = d.drafts;
            move || {
                drafts.with(|drafts| {
                    if drafts.is_empty() {
                        "⌄  Run configurations"
                    } else if drafts.iter().all(Draft::flutter) {
                        "⌄  Flutter"
                    } else if drafts.iter().all(|draft| !draft.flutter()) {
                        "⌄  Dart"
                    } else {
                        "⌄  Run configurations"
                    }
                })
            }
        })
        .style(|s| s.padding_left(10.0).height(24.0).font_bold()),
        configuration_list,
        keyboard.control(
            label(|| "Edit configuration templates…")
                .on_click_stop(move |_| template_link.choose_template(0))
                .style(move |s| {
                    s.height(30.0)
                        .items_center()
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                }),
        ),
        floem::views::stack_from_iter(template_rows.collect::<Vec<_>>()).style(
            move |s| {
                s.flex_col()
                    .width_full()
                    .apply_if(!templates_mode.get(), |s| s.hide())
            },
        ),
    ))
    .style(move |s| {
        s.width(210.0)
            .height_full()
            .flex_col()
            .flex_shrink(0.0_f32)
            .padding_horiz(10.0)
            .border_right(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    })
}
