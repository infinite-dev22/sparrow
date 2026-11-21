//! Shortcut capture and key binding editor.

use std::sync::Arc;

use floem::{
    View,
    event::{Event, EventListener},
    reactive::{
        Memo, ReadSignal, SignalGet, SignalUpdate, SignalWith, create_effect,
    },
    style::CursorStyle,
    views::{Decorators, container, dyn_stack, label, stack, text},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    keypress::{KeyPressData, keymap::KeyMapPress},
};

use super::KeymapPicker;

pub(super) fn keyboard_picker_view(
    picker: KeymapPicker,
    ui_line_height: Memo<f64>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let picker_cmd = picker.cmd;
    let view = container(
        stack((
            label(move || {
                picker_cmd.with(|cmd| {
                    cmd.as_ref()
                        .map(|cmd| {
                            cmd.kind
                                .desc()
                                .map(|desc| desc.to_string())
                                .unwrap_or_else(|| cmd.kind.str().replace('_', " "))
                        })
                        .unwrap_or_default()
                })
            }),
            dyn_stack(
                move || {
                    picker
                        .keys
                        .get()
                        .iter()
                        .map(|(key, _)| key.label())
                        .filter(|l| !l.is_empty())
                        .enumerate()
                        .collect::<Vec<(usize, String)>>()
                },
                |(i, k)| (*i, k.clone()),
                move |(_, key)| {
                    text(key.clone()).style(move |s| {
                        s.padding_horiz(5.0)
                            .padding_vert(1.0)
                            .margin_right(5.0)
                            .height(ui_line_height.get() as f32)
                            .border(1.0)
                            .border_radius(6.0)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                    })
                },
            )
            .style(move |s| {
                let config = config.get();
                s.items_center()
                    .justify_center()
                    .width_pct(100.0)
                    .margin_top(20.0)
                    .height((ui_line_height.get() as f32) * 1.2)
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(config.color(SparrowColor::SPARROW_BORDER))
                    .background(config.color(SparrowColor::EDITOR_BACKGROUND))
            }),
            stack((
                text("Save")
                    .style(move |s| {
                        let config = config.get();
                        s.width(100.0)
                            .justify_center()
                            .padding_vert(8.0)
                            .border(1.0)
                            .border_radius(6.0)
                            .border_color(config.color(SparrowColor::SPARROW_BORDER))
                            .hover(|s| {
                                s.cursor(CursorStyle::Pointer).background(
                                    config.color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            })
                            .active(|s| {
                                s.background(config.color(
                                    SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ))
                            })
                    })
                    .on_click_stop(move |_| {
                        let keymap = picker.keymap.get_untracked();
                        if let Some(keymap) = keymap {
                            let keys = picker.keys.get_untracked();
                            picker.keymap.set(None);
                            KeyPressData::update_file(
                                &keymap,
                                &keys
                                    .iter()
                                    .map(|(key, _)| key.clone())
                                    .collect::<Vec<KeyMapPress>>(),
                            );
                        }
                    }),
                text("Cancel")
                    .style(move |s| {
                        let config = config.get();
                        s.margin_left(20.0)
                            .width(100.0)
                            .justify_center()
                            .padding_vert(8.0)
                            .border(1.0)
                            .border_radius(6.0)
                            .border_color(config.color(SparrowColor::SPARROW_BORDER))
                            .hover(|s| {
                                s.cursor(CursorStyle::Pointer).background(
                                    config.color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            })
                            .active(|s| {
                                s.background(config.color(
                                    SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                ))
                            })
                    })
                    .on_click_stop(move |_| {
                        picker.keymap.set(None);
                    }),
            ))
            .style(move |s| {
                let config = config.get();
                s.items_center()
                    .justify_center()
                    .width_pct(100.0)
                    .margin_top(20.0)
                    .border_color(config.color(SparrowColor::SPARROW_BORDER))
            }),
        ))
        .on_event_stop(EventListener::PointerDown, |_| {})
        .style(move |s| {
            let config = config.get();
            s.items_center()
                .flex_col()
                .padding(20.0)
                .width(400.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(config.color(SparrowColor::SPARROW_BORDER))
                .background(config.color(SparrowColor::PANEL_BACKGROUND))
        }),
    )
    .keyboard_navigable()
    .on_event_stop(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            if let Some(keypress) = KeyPressData::keypress(key_event) {
                if let Some(keypress) = keypress.keymap_press() {
                    picker.keys.update(|keys| {
                        if let Some((last_key, last_key_confirmed)) = keys.last() {
                            if !*last_key_confirmed && last_key.is_modifiers() {
                                keys.pop();
                            }
                        }
                        if keys.len() == 2 {
                            keys.clear();
                        }
                        keys.push((keypress, false));
                    })
                }
            }
        }
    })
    .on_event_stop(EventListener::KeyUp, move |event| {
        if let Event::KeyUp(_key_event) = event {
            picker.keys.update(|keys| {
                if let Some((_last_key, last_key_confirmed)) = keys.last_mut() {
                    *last_key_confirmed = true;
                }
            })
        }
    })
    .style(move |s| {
        s.absolute()
            .size_pct(100.0, 100.0)
            .items_center()
            .justify_center()
            .apply_if(picker.keymap.with(|keymap| keymap.is_none()), |s| s.hide())
    })
    .debug_name("keyboard picker");

    let id = view.id();
    create_effect(move |_| {
        if picker.keymap.with(|k| k.is_some()) {
            id.request_focus();
        }
    });

    view
}
