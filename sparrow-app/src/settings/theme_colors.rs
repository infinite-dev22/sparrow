//! Theme color settings and color override rows.

use std::{collections::BTreeMap, rc::Rc, time::Duration};

use floem::{
    View,
    action::{TimerToken, exec_after},
    reactive::{
        Memo, Scope, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_memo, create_rw_signal,
    },
    style::CursorStyle,
    text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    views::{
        Decorators, VirtualVector, container, empty, scroll::scroll, stack, text,
        virtual_stack,
    },
};
use lapce_xi_rope::Rope;
use sparrow_core::buffer::rope_text::RopeText;

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    main_split::Editors,
    widgets::text_input::TextInputBuilder,
    window_tab::CommonData,
};

struct BTreeMapVirtualList(BTreeMap<String, String>);

impl VirtualVector<(String, String)> for BTreeMapVirtualList {
    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (String, String)> {
        Box::new(
            self.0
                .iter()
                .enumerate()
                .filter_map(|(index, (k, v))| {
                    if range.contains(&index) {
                        Some((k.to_string(), v.to_string()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .into_iter(),
        )
    }
}

fn color_section_list(
    kind: &str,
    header: &str,
    list: impl Fn() -> BTreeMap<String, String> + 'static,
    max_width: Memo<f64>,
    text_height: Memo<f64>,
    editors: Editors,
    common: Rc<CommonData>,
) -> impl View {
    let config = common.config;

    let kind = kind.to_string();
    stack((
        text(header).style(|s| {
            s.margin_top(10)
                .margin_horiz(20)
                .font_bold()
                .line_height(2.0)
        }),
        virtual_stack(
            move || BTreeMapVirtualList(list()),
            move |(key, _)| key.to_owned(),
            move |(key, value)| {
                let cx = Scope::current();
                let text_input_view = TextInputBuilder::new()
                    .value(value.clone())
                    .build(cx, editors, common.clone());
                let doc = text_input_view.doc_signal();

                {
                    let kind = kind.clone();
                    let key = key.clone();
                    let doc = text_input_view.doc_signal();
                    create_effect(move |_| {
                        let doc = doc.get_untracked();
                        let config = config.get();
                        let current = doc.buffer.with_untracked(|b| b.to_string());

                        let value = match kind.as_str() {
                            "base" => config.color_theme.base.get(&key),
                            "ui" => config.color_theme.ui.get(&key),
                            "syntax" => config.color_theme.syntax.get(&key),
                            _ => None,
                        };

                        if let Some(value) = value {
                            if value != &current {
                                doc.reload(Rope::from(value.to_string()), true);
                            }
                        }
                    });
                }

                {
                    let timer = create_rw_signal(TimerToken::INVALID);
                    let kind = kind.clone();
                    let field = key.clone();
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
                        let token =
                            exec_after(Duration::from_millis(500), move |token| {
                                if let Some(timer) = timer.try_get_untracked() {
                                    if timer == token {
                                        let value =
                                            buffer.with_untracked(|b| b.to_string());

                                        let config = config.get_untracked();
                                        let default = match kind.as_str() {
                                            "base" => config
                                                .default_color_theme()
                                                .base
                                                .get(&field),
                                            "ui" => config
                                                .default_color_theme()
                                                .ui
                                                .get(&field),
                                            "syntax" => config
                                                .default_color_theme()
                                                .syntax
                                                .get(&field),
                                            _ => None,
                                        };

                                        if default != Some(&value) {
                                            let value = serde::Serialize::serialize(
                                                &value,
                                                toml_edit::ser::ValueSerializer::new(
                                                ),
                                            )
                                            .ok();

                                            if let Some(value) = value {
                                                SparrowConfig::update_file(
                                                    &format!("color-theme.{kind}"),
                                                    &field,
                                                    value,
                                                );
                                            }
                                        } else {
                                            SparrowConfig::reset_setting(
                                                &format!("color-theme.{kind}"),
                                                &field,
                                            );
                                        }
                                    }
                                }
                            });
                        timer.set(token);

                        rev
                    });
                }

                let local_kind = kind.clone();
                let local_key = key.clone();
                stack((
                    text(&key).style(move |s| {
                        s.width(max_width.get()).margin_left(20).margin_right(10)
                    }),
                    text_input_view.keyboard_navigable().style(move |s| {
                        s.width(150.0)
                            .margin_vert(6)
                            .border(1)
                            .border_radius(6)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                    }),
                    empty().style(move |s| {
                        let size = text_height.get() + 12.0;
                        let config = config.get();
                        let color = match local_kind.as_str() {
                            "base" => config.color.base.get(&local_key),
                            "ui" => config.color.ui.get(&local_key).copied(),
                            "syntax" => config.color.syntax.get(&local_key).copied(),
                            _ => None,
                        };
                        s.border(1)
                            .border_radius(6)
                            .size(size, size)
                            .margin_left(10)
                            .border_color(config.color(SparrowColor::SPARROW_BORDER))
                            .background(color.unwrap_or_else(|| {
                                config.color(SparrowColor::EDITOR_FOREGROUND)
                            }))
                    }),
                    {
                        let kind = kind.clone();
                        let key = key.clone();
                        let local_key = key.clone();
                        let local_kind = kind.clone();
                        text("Reset")
                            .on_click_stop(move |_| {
                                SparrowConfig::reset_setting(
                                    &format!("color-theme.{local_kind}"),
                                    &local_key,
                                );
                            })
                            .style(move |s| {
                                let doc = doc.get_untracked();
                                let config = config.get();
                                let buffer = doc.buffer;
                                let content = buffer.with(|b| b.to_string());

                                let same = match kind.as_str() {
                                    "base" => {
                                        config.default_color_theme().base.get(&key)
                                            == Some(&content)
                                    }
                                    "ui" => {
                                        config.default_color_theme().ui.get(&key)
                                            == Some(&content)
                                    }
                                    "syntax" => {
                                        config.default_color_theme().syntax.get(&key)
                                            == Some(&content)
                                    }
                                    _ => false,
                                };

                                s.margin_left(10)
                                    .padding(6)
                                    .cursor(CursorStyle::Pointer)
                                    .border(1)
                                    .border_radius(6)
                                    .border_color(
                                        config.color(SparrowColor::SPARROW_BORDER),
                                    )
                                    .apply_if(same, |s| s.hide())
                                    .active(|s| {
                                        s.background(
                                            config.color(
                                                SparrowColor::PANEL_BACKGROUND,
                                            ),
                                        )
                                    })
                            })
                    },
                ))
                .style(|s| s.items_center())
            },
        )
        .item_size_fixed(move || text_height.get() + 24.0)
        .style(|s| s.flex_col().padding_right(20)),
    ))
    .style(|s| s.flex_col())
}

pub fn theme_color_settings_view(
    editors: Editors,
    common: Rc<CommonData>,
) -> impl View {
    let config = common.config;

    let text_height = create_memo(move |_| {
        let mut text_layout = TextLayout::new();
        let config = config.get();
        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.ui.font_family).collect();
        let attrs = Attrs::new()
            .family(&family)
            .font_size(config.ui.font_size() as f32);
        let attrs_list = AttrsList::new(attrs);
        text_layout.set_text("W", attrs_list, None);
        text_layout.size().height
    });

    let max_width = create_memo(move |_| {
        let mut text_layout = TextLayout::new();
        let config = config.get();
        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.ui.font_family).collect();
        let attrs = Attrs::new()
            .family(&family)
            .font_size(config.ui.font_size() as f32);
        let attrs_list = AttrsList::new(attrs);

        let mut max_width = 0.0;
        for key in config.color_theme.ui.keys() {
            text_layout.set_text(key, attrs_list.clone(), None);
            let width = text_layout.size().width;
            if width > max_width {
                max_width = width;
            }
        }
        for key in config.color_theme.syntax.keys() {
            text_layout.set_text(key, attrs_list.clone(), None);
            let width = text_layout.size().width;
            if width > max_width {
                max_width = width;
            }
        }
        max_width
    });

    let cx = Scope::current();
    let search_editor = editors.make_local(cx, common.clone());
    let search_editor_id = search_editor.id();
    let buffer = search_editor.doc_signal().get_untracked().buffer;

    scroll(
        stack((
            container({
                TextInputBuilder::new()
                    .build_editor(search_editor)
                    .placeholder(|| "Search Settings".to_string())
                    .keyboard_navigable()
                    .style(move |s| {
                        s.width_pct(100.0)
                            .border_radius(6.0)
                            .border(1.0)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                    })
                    .request_focus(|| {})
            })
            .style(|s| s.padding_vert(20.0).padding_horiz(20.0)),
            color_section_list(
                "base",
                "Base Colors",
                move || {
                    let filter = buffer.get().text().to_string();
                    config.with(|c| {
                        c.color_theme
                            .base
                            .0
                            .iter()
                            .filter_map(|x| {
                                if x.0.contains(&filter) {
                                    Some((x.0.clone(), x.1.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect::<BTreeMap<String, String>>()
                    })
                },
                max_width,
                text_height,
                editors,
                common.clone(),
            ),
            color_section_list(
                "syntax",
                "Syntax Colors",
                move || {
                    let filter = buffer.get().text().to_string();
                    config.with(|c| {
                        c.color_theme
                            .syntax
                            .iter()
                            .filter_map(|x| {
                                if x.0.contains(&filter) {
                                    Some((x.0.clone(), x.1.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect::<BTreeMap<String, String>>()
                    })
                },
                max_width,
                text_height,
                editors,
                common.clone(),
            ),
            color_section_list(
                "ui",
                "UI Colors",
                move || {
                    let filter = buffer.get().text().to_string();
                    config.with(|c| {
                        c.color_theme
                            .ui
                            .iter()
                            .filter_map(|x| {
                                if x.0.contains(&filter) {
                                    Some((x.0.clone(), x.1.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect::<BTreeMap<String, String>>()
                    })
                },
                max_width,
                text_height,
                editors,
                common.clone(),
            ),
        ))
        .style(|s| s.flex_col()),
    )
    .style(|s| s.absolute().size_full())
    .debug_name("Theme Color Settings")
    .on_cleanup(move || {
        editors.remove(search_editor_id);
    })
}
