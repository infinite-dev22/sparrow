//! Settings dialog row with scoped overrides and reset actions.

use super::{Edit, HighlightMatcher, Pending, Type, initial};
use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};
use crate::settings::{SettingsItem, SettingsValue};
use crate::widgets::{focus_text::focus_text, modal_focus::ModalFocus};
use floem::{
    IntoView, View,
    reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, label, stack},
};

pub(super) fn setting_row(
    item: SettingsItem,
    pending: RwSignal<Pending>,
    keyboard: ModalFocus,
    project_scope: RwSignal<bool>,
    highlight: Memo<Option<HighlightMatcher>>,
    config: floem::reactive::ReadSignal<std::sync::Arc<SparrowConfig>>,
) -> impl View {
    let (original, kind) = initial(&item.value).unwrap();
    let key = (item.kind.clone(), item.field.clone());
    let raw = pending
        .with_untracked(|p| p.get(&key).map(|v| v.raw.clone()))
        .unwrap_or_else(|| original.clone());
    let reset_key = key.clone();
    let reset_value = original.clone();
    let reset_key_label = key.clone();
    let value = floem::reactive::create_rw_signal(raw);
    Scope::current().create_effect(move |last: Option<String>| {
        let raw = value.get();
        if last.is_some() {
            pending.update(|p| {
                if raw == original {
                    p.remove(&key);
                } else {
                    p.insert(
                        key.clone(),
                        Edit {
                            raw: raw.clone(),
                            kind: kind.clone(),
                        },
                    );
                }
            });
        }
        raw
    });
    let name = item
        .name
        .split_once(": ")
        .map(|(_, title)| title)
        .unwrap_or(&item.name)
        .to_owned();
    let is_boolean = matches!(item.value, SettingsValue::Bool(_));
    let checkbox_name = name.clone();
    let control = match item.value {
        SettingsValue::Bool(_) => keyboard
            .control(crate::widgets::labeled_checkbox(
                move || value.with(|value| value == "true"),
                move || {
                    if highlight.with(Option::is_some) {
                        if value.with(|value| value == "true") {
                            "Enabled".into()
                        } else {
                            "Disabled".into()
                        }
                    } else {
                        checkbox_name.clone()
                    }
                },
                move |checked| value.set(checked.to_string()),
                config,
            ))
            .into_any(),
        SettingsValue::Dropdown(info) => keyboard
            .control(
                crate::widgets::dropdown(value, info.items.clone(), config)
                    .style(|s| s.width(220.0).max_width_full()),
            )
            .into_any(),
        SettingsValue::String(_)
            if item.kind == "core"
                && ["flutter-sdk-path", "dart-sdk-path"]
                    .contains(&item.field.as_str()) =>
        {
            let input = keyboard
                .input(value, config)
                .style(move |s| s.width(400.0).max_width_full());
            let input_id = input.id();
            let browser = keyboard.clone();
            stack((
                input,
                keyboard.icon(
                    || SparrowIcons::DIRECTORY,
                    move || {
                        let mut options = crate::widgets::file_picker::FileDialogOptions::new()
                            .title("Choose SDK directory")
                            .select_directories();
                        let current =
                            std::path::PathBuf::from(value.get_untracked());
                        if current.is_dir() {
                            options = options.force_starting_directory(current);
                        }
                        browser.open_file(options, config, move |selection| {
                            if let Some(path) = selection.and_then(|selection| {
                                selection.path.first().cloned()
                            }) {
                                value.set(path.display().to_string());
                            }
                            if !input_id.is_hidden_recursive() {
                                input_id.request_focus();
                            }
                        })
                    },
                    || false,
                    || false,
                    || "Choose SDK directory",
                    config,
                ),
            ))
            .style(|s| s.items_center().gap(6.0))
            .into_any()
        }
        _ => keyboard
            .input(value, config)
            .style(move |s| s.width(320.0).max_width_full())
            .into_any(),
    };
    let reset = keyboard.control(
        label(move || {
            if pending.with(|pending| {
                pending
                    .get(&reset_key_label)
                    .is_some_and(|edit| matches!(edit.kind, Type::Reset))
            }) {
                "Will inherit IDE value on Apply"
            } else {
                "Use IDE value"
            }
        })
        .on_click_stop(move |_| {
            pending.update(|pending| {
                pending.insert(
                    reset_key.clone(),
                    Edit {
                        raw: reset_value.clone(),
                        kind: Type::Reset,
                    },
                );
            })
        })
        .style(move |s| {
            s.height(24.0)
                .items_center()
                .color(config.get().color(SparrowColor::EDITOR_LINK))
                .apply_if(!project_scope.get(), |s| s.hide())
        }),
    );
    let qualified_name = item.name.clone();
    let gap = super::tokens::length("navigation", "tree_icon_gap");
    let field = stack((
        focus_text(
            {
                let qualified_name = qualified_name.clone();
                move || {
                    if highlight.with(Option::is_some) {
                        qualified_name.clone()
                    } else {
                        name.clone()
                    }
                }
            },
            move || {
                highlight.with(|matcher| {
                    matcher.as_ref().map_or_else(Vec::new, |matcher| {
                        matcher
                            .0
                            .find_iter(&qualified_name)
                            .flat_map(|matched| {
                                matched
                                    .as_str()
                                    .char_indices()
                                    .map(move |(index, _)| matched.start() + index)
                            })
                            .collect()
                    })
                })
            },
            move || config.get().color(SparrowColor::EDITOR_LINK),
        )
        .style(move |s| {
            s.max_width_full()
                .min_width(0)
                .apply_if(is_boolean && highlight.with(Option::is_none), |s| {
                    s.hide()
                })
        }),
        control,
        reset,
    ))
    .style(move |s| {
        s.items_center()
            .width_full()
            .gap(gap)
            .flex_wrap(floem::style::FlexWrap::Wrap)
    });
    let item_description_empty = item.description.is_empty();
    stack((
        field,
        label(move || item.description.clone()).style(move |s| {
            s.max_width_full()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .apply_if(is_boolean && item_description_empty, |s| s.hide())
        }),
    ))
    .style(move |s| s.flex_col().width_full().gap(gap).padding_bottom(gap))
}
