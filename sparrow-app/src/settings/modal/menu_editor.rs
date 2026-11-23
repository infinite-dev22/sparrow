//! Edit the ordered list shared by the main menu and toolbar Quick Actions.

use std::sync::Arc;

use floem::{
    View,
    reactive::{ReadSignal, RwSignal, Scope, SignalUpdate, SignalWith},
    style::FlexWrap,
    views::{Decorators, dyn_stack, label, stack},
};

use crate::{
    config::{SparrowConfig, icon::SparrowIcons},
    menus::{QUICK_ACTIONS, quick_action},
    widgets::{ButtonKind, button, dropdown, modal_focus::ModalFocus},
};

use super::{Edit, Pending, Type, form, tokens};

fn key() -> (String, String) {
    ("ui".into(), "quick-actions".into())
}

fn current(
    pending: RwSignal<Pending>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> Vec<String> {
    pending.with(|pending| {
        pending
            .get(&key())
            .and_then(|edit| serde_json::from_str(&edit.raw).ok())
            .unwrap_or_else(|| config.with(|config| config.ui.quick_actions.clone()))
    })
}

fn update(pending: RwSignal<Pending>, baseline: &[String], actions: Vec<String>) {
    pending.update(|pending| {
        if actions == baseline {
            pending.remove(&key());
        } else {
            pending.insert(
                key(),
                Edit {
                    raw: serde_json::to_string(&actions)
                        .expect("serializable command IDs"),
                    kind: Type::StringList,
                },
            );
        }
    });
}

pub(super) fn view(
    keyboard: ModalFocus,
    pending: RwSignal<Pending>,
    settings: ReadSignal<Arc<SparrowConfig>>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let selected = Scope::current().create_rw_signal(QUICK_ACTIONS[0].0.to_owned());
    let gap = tokens::length("navigation", "tree_icon_gap");
    let choices = QUICK_ACTIONS
        .iter()
        .map(|(title, _)| title.to_string())
        .collect::<Vec<_>>();
    let picker = keyboard
        .control(dropdown(selected, choices, config))
        .style(|s| s.max_width_full().min_width(0));
    let candidate = move || {
        selected.with(|title| {
            QUICK_ACTIONS
                .iter()
                .find(|(label, _)| label == title)
                .map(|(_, command)| <&'static str>::from(command))
        })
    };
    let unavailable = move || {
        candidate().is_none_or(|id| {
            current(pending, settings).iter().any(|entry| entry == id)
        })
    };
    let add = keyboard.control_if(
        button(|| "Add action", ButtonKind::Outlined, config)
            .disabled(unavailable)
            .on_click_stop(move |_| {
                if let Some(id) = candidate() {
                    let mut actions = current(pending, settings);
                    if !actions.iter().any(|entry| entry == id) {
                        actions.push(id.into());
                        settings.with(|config| {
                            update(pending, &config.ui.quick_actions, actions)
                        });
                    }
                }
            }),
        move || !unavailable(),
    );
    let clear_disabled = move || current(pending, settings).is_empty();
    let clear = keyboard.control_if(
        button(|| "Clear list", ButtonKind::Plain, config)
            .on_click_stop(move |_| {
                settings.with(|config| {
                    update(pending, &config.ui.quick_actions, Vec::new())
                })
            })
            .disabled(clear_disabled),
        move || !clear_disabled(),
    );
    let rows = dyn_stack(
        move || {
            current(pending, settings)
                .into_iter()
                .enumerate()
                .collect::<Vec<_>>()
        },
        |(index, id)| (*index, id.clone()),
        move |(index, id)| {
            let title = quick_action(&id)
                .map(|(title, _)| title.to_owned())
                .unwrap_or(id);
            let icon = |glyph, tip, offset: isize| {
                keyboard.icon(
                    move || glyph,
                    move || {
                        let mut actions = current(pending, settings);
                        if offset == 0 {
                            if index < actions.len() {
                                actions.remove(index);
                            }
                        } else if let Some(target) = index.checked_add_signed(offset)
                            && target < actions.len()
                            && index < actions.len()
                        {
                            actions.swap(index, target);
                        }
                        settings.with(|config| {
                            update(pending, &config.ui.quick_actions, actions)
                        });
                    },
                    || false,
                    move || {
                        offset != 0
                            && index.checked_add_signed(offset).is_none_or(
                                |target| target >= current(pending, settings).len(),
                            )
                    },
                    move || tip,
                    config,
                )
            };
            stack((
                label(move || title.clone())
                    .style(|s| s.flex_grow(1.0_f32).min_width(0).text_ellipsis()),
                icon(SparrowIcons::ITEM_OPENED, "Move up", -1),
                icon(SparrowIcons::ITEM_CLOSED, "Move down", 1),
                icon(SparrowIcons::CLOSE, "Remove action", 0),
            ))
            .style(move |s| {
                s.items_center()
                    .width_full()
                    .gap(gap)
                    .flex_wrap(FlexWrap::Wrap)
            })
        },
    )
    .style(|s| s.flex_col().width_full());
    stack((
        form::section_title("Quick Actions", config),
        label(
            || "Choose the commands and their order in the main menu and toolbar.",
        ),
        stack((picker, add, clear))
            .style(move |s| s.items_center().gap(gap).flex_wrap(FlexWrap::Wrap)),
        rows,
        label(|| "Add an action to show Quick Actions in the toolbar.")
            .style(move |s| s.apply_if(!clear_disabled(), |s| s.hide())),
    ))
    .style(move |s| s.flex_col().gap(gap).width_full().min_width(0))
}
