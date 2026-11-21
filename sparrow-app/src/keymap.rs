use std::rc::Rc;

use floem::{
    View,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal,
    },
    views::{
        Decorators, container, dyn_stack, stack, text, virtual_stack,
    },
};
use sparrow_core::mode::Modes;

use crate::{
    command::SparrowCommand,
    config::color::SparrowColor,
    keypress::keymap::{KeyMap, KeyMapPress},
    main_split::Editors,
    widgets::text_input::TextInputBuilder,
    window_tab::CommonData,
};

mod shortcut_picker;

use shortcut_picker::keyboard_picker_view;
use crate::widgets::sparrow_scroll;

#[derive(Clone)]
pub struct KeymapPicker {
    cmd: RwSignal<Option<SparrowCommand>>,
    keymap: RwSignal<Option<KeyMap>>,
    keys: RwSignal<Vec<(KeyMapPress, bool)>>,
}

pub fn keymap_view(editors: Editors, common: Rc<CommonData>) -> impl View {
    let config = common.config;
    let keypress = common.keypress;
    let ui_line_height_memo = common.ui_line_height;
    let ui_line_height = move || ui_line_height_memo.get() * 1.2;
    let modal = create_memo(move |_| config.get().core.modal);
    let picker = KeymapPicker {
        cmd: create_rw_signal(None),
        keymap: create_rw_signal(None),
        keys: create_rw_signal(Vec::new()),
    };

    let cx = Scope::current();
    let text_input_view = TextInputBuilder::new().build(cx, editors, common.clone());
    let doc = text_input_view.doc_signal();

    let items = move || {
        let doc = doc.get();
        let pattern = doc.buffer.with(|b| b.to_string().to_lowercase());
        let keypress = keypress.get();
        let mut items = keypress
            .commands_with_keymap
            .iter()
            .filter_map(|keymap| {
                let cmd = keypress.commands.get(&keymap.command).cloned()?;

                let cmd_name_contains_pattern =
                    cmd.kind.str().replace('_', " ").contains(&pattern);
                let cmd_desc_contains_pattern = cmd
                    .kind
                    .desc()
                    .map(|desc| desc.to_lowercase().contains(&pattern))
                    .unwrap_or(false);
                let shortcut_contains_pattern = keymap
                    .key
                    .iter()
                    .any(|k| k.label().trim().to_lowercase().contains(&pattern));
                let when_contains_pattern = keymap
                    .when
                    .as_ref()
                    .map(|when| when.to_lowercase().contains(&pattern))
                    .unwrap_or(false);

                if cmd_name_contains_pattern
                    || cmd_desc_contains_pattern
                    || shortcut_contains_pattern
                    || when_contains_pattern
                {
                    Some((cmd, Some(keymap.clone())))
                } else {
                    None
                }
            })
            .collect::<im::Vector<(SparrowCommand, Option<KeyMap>)>>();
        items.extend(keypress.commands_without_keymap.iter().filter_map(|cmd| {
            let match_pattern = cmd.kind.str().replace('_', " ").contains(&pattern)
                || cmd
                    .kind
                    .desc()
                    .map(|desc| desc.to_lowercase().contains(&pattern))
                    .unwrap_or(false);
            if !match_pattern {
                return None;
            }
            Some((cmd.clone(), None))
        }));
        items
            .into_iter()
            .enumerate()
            .collect::<im::Vector<(usize, (SparrowCommand, Option<KeyMap>))>>()
    };

    let view_fn =
        move |(i, (cmd, keymap)): (usize, (SparrowCommand, Option<KeyMap>))| {
            let local_keymap = keymap.clone();
            let local_cmd = cmd.clone();
            stack((
                container(
                    text(
                        cmd.kind
                            .desc()
                            .map(|desc| desc.to_string())
                            .unwrap_or_else(|| cmd.kind.str().replace('_', " ")),
                    )
                    .style(|s| {
                        s.text_ellipsis()
                            .absolute()
                            .items_center()
                            .min_width(0.0)
                            .padding_horiz(10.0)
                            .size_pct(100.0, 100.0)
                    }),
                )
                .style(move |s| {
                    s.height_pct(100.0)
                        .min_width(0.0)
                        .flex_basis(0.0)
                        .flex_grow(1.0f32)
                        .border_right(1.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                }),
                {
                    let keymap = keymap.clone();
                    dyn_stack(
                        move || {
                            keymap
                                .as_ref()
                                .map(|keymap| {
                                    keymap
                                        .key
                                        .iter()
                                        .map(|key| key.label())
                                        .filter(|l| !l.is_empty())
                                        .collect::<Vec<String>>()
                                })
                                .unwrap_or_default()
                        },
                        |k| k.clone(),
                        move |key| {
                            text(key.clone()).style(move |s| {
                                s.padding_horiz(5.0)
                                    .padding_vert(1.0)
                                    .margin_right(5.0)
                                    .border(1.0)
                                    .border_radius(3.0)
                                    .border_color(
                                        config
                                            .get()
                                            .color(SparrowColor::SPARROW_BORDER),
                                    )
                            })
                        },
                    )
                    .style(move |s| {
                        s.items_center()
                            .padding_horiz(10.0)
                            .min_width(200.0)
                            .height_pct(100.0)
                            .border_right(1.0)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                    })
                },
                {
                    let keymap = keymap.clone();
                    let bits = [
                        (Modes::INSERT, "Insert"),
                        (Modes::NORMAL, "Normal"),
                        (Modes::VISUAL, "Visual"),
                        (Modes::TERMINAL, "Terminal"),
                    ];
                    let modes = keymap
                        .as_ref()
                        .map(|keymap| {
                            bits.iter()
                                .filter_map(|(bit, mode)| {
                                    if keymap.modes.contains(*bit) {
                                        Some(mode.to_string())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default();
                    dyn_stack(
                        move || modes.clone(),
                        |m| m.clone(),
                        move |mode| {
                            text(mode.clone()).style(move |s| {
                                s.padding_horiz(5.0)
                                    .padding_vert(1.0)
                                    .margin_right(5.0)
                                    .border(1.0)
                                    .border_radius(3.0)
                                    .border_color(
                                        config
                                            .get()
                                            .color(SparrowColor::SPARROW_BORDER),
                                    )
                            })
                        },
                    )
                    .style(move |s| {
                        s.items_center()
                            .padding_horiz(10.0)
                            .min_width(200.0)
                            .height_pct(100.0)
                            .border_right(1.0)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                            .apply_if(!modal.get(), |s| s.hide())
                    })
                },
                container(
                    text(
                        keymap
                            .as_ref()
                            .and_then(|keymap| keymap.when.clone())
                            .unwrap_or_default(),
                    )
                    .style(|s| {
                        s.text_ellipsis()
                            .absolute()
                            .items_center()
                            .min_width(0.0)
                            .padding_horiz(10.0)
                            .size_pct(100.0, 100.0)
                    }),
                )
                .style(move |s| {
                    s.height_pct(100.0)
                        .min_width(0.0)
                        .flex_basis(0.0)
                        .flex_grow(1.0f32)
                }),
            ))
            .on_click_stop(move |_| {
                let keymap = if let Some(keymap) = local_keymap.clone() {
                    keymap
                } else {
                    KeyMap {
                        command: local_cmd.kind.str().to_string(),
                        key: Vec::new(),
                        modes: Modes::empty(),
                        when: None,
                    }
                };
                picker.keymap.set(Some(keymap));
                picker.cmd.set(Some(local_cmd.clone()));
                picker.keys.update(|keys| {
                    keys.clear();
                });
            })
            .style(move |s| {
                let config = config.get();
                s.items_center()
                    .height(ui_line_height() as f32)
                    .width_pct(100.0)
                    .apply_if(i % 2 > 0, |s| {
                        s.background(config.color(SparrowColor::EDITOR_CURRENT_LINE))
                    })
                    .border_bottom(1.0)
                    .border_color(config.color(SparrowColor::SPARROW_BORDER))
            })
        };

    stack((
        container(
            text_input_view
                .placeholder(|| "Search Key Bindings".to_string())
                .keyboard_navigable()
                .request_focus(|| {})
                .style(move |s| {
                    s.width_pct(100.0)
                        .border_radius(6.0)
                        .border(1.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                }),
        )
        .style(|s| s.padding_bottom(10.0).width_pct(100.0)),
        stack((
            container(text("Command").style(move |s| {
                s.text_ellipsis().padding_horiz(10.0).min_width(0.0)
            }))
            .style(move |s| {
                s.items_center()
                    .height_pct(100.0)
                    .min_width(0.0)
                    .flex_basis(0.0)
                    .flex_grow(1.0f32)
                    .border_right(1.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            }),
            text("Key Binding").style(move |s| {
                s.width(200.0)
                    .items_center()
                    .padding_horiz(10.0)
                    .height_pct(100.0)
                    .border_right(1.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            }),
            text("Modes").style(move |s| {
                s.width(200.0)
                    .items_center()
                    .padding_horiz(10.0)
                    .height_pct(100.0)
                    .border_right(1.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                    .apply_if(!modal.get(), |s| s.hide())
            }),
            container(text("When").style(move |s| {
                s.text_ellipsis().padding_horiz(10.0).min_width(0.0)
            }))
            .style(move |s| {
                s.items_center()
                    .height_pct(100.0)
                    .min_width(0.0)
                    .flex_basis(0.0)
                    .flex_grow(1.0f32)
            }),
        ))
        .style(move |s| {
            let config = config.get();
            s.font_bold()
                .height(ui_line_height() as f32)
                .width_pct(100.0)
                .border_top(1.0)
                .border_bottom(1.0)
                .border_color(config.color(SparrowColor::SPARROW_BORDER))
                .background(config.color(SparrowColor::EDITOR_CURRENT_LINE))
        }),
        container(
            sparrow_scroll(
                virtual_stack(
                    items,
                    |(i, (cmd, keymap)): &(
                        usize,
                        (SparrowCommand, Option<KeyMap>),
                    )| { (*i, cmd.kind.str(), keymap.clone()) },
                    view_fn,
                )
                .item_size_fixed(ui_line_height)
                .style(|s| s.flex_col().width_pct(100.0)),
            )
            .style(|s| s.absolute().size_pct(100.0, 100.0)),
        )
        .style(|s| s.width_pct(100.0).flex_basis(0.0).flex_grow(1.0f32)),
        keyboard_picker_view(picker, common.ui_line_height, config),
    ))
    .style(|s| {
        s.absolute()
            .size_pct(100.0, 100.0)
            .flex_col()
            .padding_top(20.0)
            .padding_left(20.0)
            .padding_right(20.0)
    })
}
