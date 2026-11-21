//! Settings row controls selected by value type.

use std::time::Duration;

use floem::{
    IntoView, View,
    action::{TimerToken, exec_after},
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_rw_signal,
    },
    style::CursorStyle,
    views::{Decorators, container, empty, label, stack},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    main_split::Editors,
    widgets::text_input::TextInputBuilder,
};

use super::{SettingsData, SettingsItem, SettingsValue};

pub(super) fn settings_item_view(
    editors: Editors,
    settings_data: SettingsData,
    item: SettingsItem,
) -> impl View + use<> {
    let config = settings_data.common.config;

    let is_ticked = if let SettingsValue::Bool(is_ticked) = &item.value {
        Some(*is_ticked)
    } else {
        None
    };

    let timer = create_rw_signal(TimerToken::INVALID);

    let editor_value = match &item.value {
        SettingsValue::Float(n) => Some(n.to_string()),
        SettingsValue::Integer(n) => Some(n.to_string()),
        SettingsValue::String(s) => Some(s.to_string()),
        SettingsValue::Bool(_) => None,
        SettingsValue::Dropdown(_) => None,
        SettingsValue::Empty => None,
    };

    let view = {
        let item = item.clone();
        move || {
            let cx = Scope::current();
            if let Some(editor_value) = editor_value {
                let text_input_view = TextInputBuilder::new()
                    .value(editor_value)
                    .build(cx, editors, settings_data.common);

                let doc = text_input_view.doc_signal();

                let kind = item.kind.clone();
                let field = item.field.clone();
                let item_value = item.value.clone();
                create_effect(move |last| {
                    let doc = doc.get_untracked();
                    let rev = doc.buffer.with(|b| b.rev());
                    if last.is_none() {
                        return rev;
                    }
                    if last == Some(rev) {
                        return rev;
                    }
                    let kind = kind.clone();
                    let field = field.clone();
                    let buffer = doc.buffer;
                    let item_value = item_value.clone();
                    let token =
                        exec_after(Duration::from_millis(500), move |token| {
                            let Some(timer) = timer.try_get_untracked() else {
                                return;
                            };
                            if timer != token {
                                return;
                            }

                            let value = buffer.with_untracked(|b| b.to_string());
                            // FIXME: Figure out how to block certain keys in inputs and not hate myself
                            let value = value.trim();
                            let value = match &item_value {
                                SettingsValue::Float(_) => {
                                    value.parse::<f64>().ok().and_then(|v| {
                                        serde::Serialize::serialize(
                                            &v,
                                            toml_edit::ser::ValueSerializer::new(),
                                        )
                                        .ok()
                                    })
                                }
                                SettingsValue::Integer(_) => {
                                    value.parse::<i64>().ok().and_then(|v| {
                                        serde::Serialize::serialize(
                                            &v,
                                            toml_edit::ser::ValueSerializer::new(),
                                        )
                                        .ok()
                                    })
                                }
                                _ => serde::Serialize::serialize(
                                    &value,
                                    toml_edit::ser::ValueSerializer::new(),
                                )
                                .ok(),
                            };

                            if let Some(value) = value {
                                SparrowConfig::update_file(&kind, &field, value);
                            }
                        });
                    timer.set(token);

                    rev
                });

                text_input_view
                    .keyboard_navigable()
                    .style(move |s| {
                        s.width(300.0).border(1.0).border_radius(6.0).border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                    })
                    .into_any()
            } else if let SettingsValue::Dropdown(dropdown) = &item.value {
                let current_value = dropdown
                    .items
                    .get(dropdown.active_index)
                    .or_else(|| dropdown.items.last())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let current_value = create_rw_signal(current_value);

                {
                    let kind = item.kind.clone();
                    let field = item.field.clone();
                    crate::widgets::dropdown(current_value, dropdown.items.clone(), config)
                        .on_accept(move |selected| {
                            current_value.set(selected.clone());
                            if let Ok(value) = serde::Serialize::serialize(
                                &selected,
                                toml_edit::ser::ValueSerializer::new(),
                            ) {
                                SparrowConfig::update_file(&kind, &field, value);
                            }
                        })
                        .style(|s| s.width(250.0).max_width_full())
                }
                .into_any()
            } else if item.header {
                label(move || item.kind.clone())
                    .style(move |s| {
                        let config = config.get();
                        s.line_height(2.0)
                            .font_bold()
                            .width_pct(100.0)
                            .padding_horiz(10.0)
                            .font_size(config.ui.font_size() as f32 + 2.0)
                            .background(config.color(SparrowColor::PANEL_BACKGROUND))
                    })
                    .into_any()
            } else {
                empty().into_any()
            }
        }
    };

    stack((
        label(move || item.name.clone()).style(move |s| {
            s.font_bold()
                .text_ellipsis()
                .min_width(0.0)
                .max_width_pct(100.0)
                .line_height(1.8)
                .font_size(config.get().ui.font_size() as f32 + 1.0)
        }),
        stack((
            label(move || item.description.clone()).style(move |s| {
                s.min_width(0.0)
                    .max_width_pct(100.0)
                    .line_height(1.8)
                    .apply_if(is_ticked.is_some(), |s| {
                        s.margin_left(config.get().ui.font_size() as f32 + 8.0)
                    })
                    .apply_if(item.header, |s| s.hide())
            }),
            if let Some(is_ticked) = is_ticked {
                let checked = create_rw_signal(is_ticked);

                let kind = item.kind.clone();
                let field = item.field.clone();
                create_effect(move |last| {
                    let checked = checked.get();
                    if last.is_none() {
                        return;
                    }
                    if let Ok(value) = serde::Serialize::serialize(
                        &checked,
                        toml_edit::ser::ValueSerializer::new(),
                    ) {
                        SparrowConfig::update_file(&kind, &field, value);
                    }
                });

                container(
                    stack((
                        crate::widgets::checkbox(move || checked.get(), config)
                            .on_update(move |value| checked.set(value)),
                        label(|| " ".to_string()).style(|s| s.line_height(1.8)),
                    ))
                    .style(|s| s.items_center()),
                )
                .on_click_stop(move |_| {
                    checked.update(|checked| {
                        *checked = !*checked;
                    });
                })
                .style(|s| {
                    s.absolute()
                        .cursor(CursorStyle::Pointer)
                        .size_pct(100.0, 100.0)
                        .items_start()
                })
            } else {
                container(empty()).style(|s| s.hide())
            },
        )),
        view().style(move |s| s.apply_if(!item.header, |s| s.margin_top(6.0))),
    ))
    .on_resize(move |rect| {
        if item.header {
            item.pos.set(rect.origin());
        }
        let old_size = item.size.get_untracked();
        let new_size = rect.size();
        if old_size != new_size {
            item.size.set(new_size);
        }
    })
    .style(|s| {
        s.flex_col()
            .padding_vert(10.0)
            .min_width_pct(100.0)
            .max_width(300.0)
    })
}
