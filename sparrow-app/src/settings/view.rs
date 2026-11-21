//! Settings editor layout and filtering.

use std::rc::Rc;

use floem::{
    View,
    peniko::kurbo::{Point, Rect, Size},
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_memo, create_rw_signal,
    },
    style::CursorStyle,
    views::{Decorators, container, dyn_stack, label, scroll::scroll, stack},
};
use indexmap::IndexMap;
use sparrow_rpc::plugin::VoltID;

use crate::{
    config::color::SparrowColor, main_split::Editors, plugin::InstalledVoltData,
    widgets::text_input::TextInputBuilder, window_tab::CommonData,
};

use super::{SettingsData, setting_row::settings_item_view};

pub fn settings_view(
    installed_plugins: RwSignal<IndexMap<VoltID, InstalledVoltData>>,
    editors: Editors,
    common: Rc<CommonData>,
) -> impl View {
    let config = common.config;

    let cx = Scope::current();
    let settings_data = SettingsData::new(cx, installed_plugins, common.clone());
    let view_settings_data = settings_data.clone();
    let plugin_kinds = settings_data.plugin_kinds;

    let search_editor = editors.make_local(cx, common);
    let search_editor_id = search_editor.id();
    let doc = search_editor.doc_signal();

    let items = settings_data.items;
    let kinds = settings_data.kinds;
    let filtered_items_signal = settings_data.filtered_items;
    create_effect(move |_| {
        let doc = doc.get();
        let pattern = doc.buffer.with(|b| b.to_string().to_lowercase());
        let plugin_items = settings_data.plugin_items.get();
        let mut items = items.get();
        if pattern.is_empty() {
            items.extend(plugin_items);
            filtered_items_signal.set(items);
            return;
        }

        let mut filtered_items = im::Vector::new();
        for item in &items {
            if item.header || item.filter_text.contains(&pattern) {
                filtered_items.push_back(item.clone());
            }
        }
        for item in plugin_items {
            if item.header || item.filter_text.contains(&pattern) {
                filtered_items.push_back(item);
            }
        }
        filtered_items_signal.set(filtered_items);
    });

    let ensure_visible = create_rw_signal(Rect::ZERO);
    let settings_content_size = create_rw_signal(Size::ZERO);
    let scroll_pos = create_rw_signal(Point::ZERO);

    let current_kind = {
        create_memo(move |_| {
            let scroll_pos = scroll_pos.get();
            let scroll_y = scroll_pos.y + 30.0;

            let plugin_kinds = plugin_kinds.get_untracked();
            for (kind, pos) in plugin_kinds.iter().rev() {
                if pos.get_untracked().y < scroll_y {
                    return kind.to_string();
                }
            }

            let kinds = kinds.get();
            for (kind, pos) in kinds.iter().rev() {
                if pos.get_untracked().y < scroll_y {
                    return kind.to_string();
                }
            }

            kinds.get(0).unwrap().0.to_string()
        })
    };

    let switcher_item = move |k: String,
                              pos: Box<dyn Fn() -> Option<RwSignal<Point>>>,
                              margin: f32| {
        let kind = k.clone();
        container(
            label(move || k.clone())
                .style(move |s| s.text_ellipsis().padding_left(margin)),
        )
        .on_click_stop(move |_| {
            if let Some(pos) = pos() {
                ensure_visible.set(
                    settings_content_size
                        .get_untracked()
                        .to_rect()
                        .with_origin(pos.get_untracked()),
                );
            }
        })
        .style(move |s| {
            let config = config.get();
            s.padding_horiz(20.0)
                .width_pct(100.0)
                .apply_if(kind == current_kind.get(), |s| {
                    s.background(
                        config.color(SparrowColor::PANEL_CURRENT_BACKGROUND),
                    )
                })
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.background(
                        config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
        })
    };

    let switcher = || {
        stack((
            dyn_stack(
                move || kinds.get().clone(),
                |(k, _)| k.clone(),
                move |(k, pos)| switcher_item(k, Box::new(move || Some(pos)), 0.0),
            )
            .style(|s| s.flex_col().width_pct(100.0)),
            stack((
                switcher_item(
                    "Plugin Settings".to_string(),
                    Box::new(move || {
                        plugin_kinds
                            .with_untracked(|k| k.get(0).map(|(_, pos)| *pos))
                    }),
                    0.0,
                ),
                dyn_stack(
                    move || plugin_kinds.get(),
                    |(k, _)| k.clone(),
                    move |(k, pos)| {
                        switcher_item(k, Box::new(move || Some(pos)), 10.0)
                    },
                )
                .style(|s| s.flex_col().width_pct(100.0)),
            ))
            .style(move |s| {
                s.width_pct(100.0)
                    .flex_col()
                    .apply_if(plugin_kinds.with(|k| k.is_empty()), |s| s.hide())
            }),
        ))
        .style(move |s| {
            s.width_pct(100.0)
                .flex_col()
                .line_height(1.8)
                .font_size(config.get().ui.font_size() as f32 + 1.0)
        })
    };

    stack((
        container({
            scroll({
                container(switcher())
                    .style(|s| s.padding_vert(20.0).width_pct(100.0))
            })
            .style(|s| s.absolute().size_pct(100.0, 100.0))
        })
        .style(move |s| {
            s.height_pct(100.0)
                .width(200.0)
                .border_right(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
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
            .style(|s| s.padding_horiz(50.0).padding_vert(20.0)),
            container({
                scroll({
                    dyn_stack(
                        move || filtered_items_signal.get(),
                        |item| {
                            (
                                item.kind.clone(),
                                item.name.clone(),
                                item.serde_value.clone(),
                            )
                        },
                        move |item| {
                            settings_item_view(
                                editors,
                                view_settings_data.clone(),
                                item,
                            )
                        },
                    )
                    .style(|s| {
                        s.flex_col()
                            .padding_horiz(50.0)
                            .min_width_pct(100.0)
                            .max_width(400.0)
                    })
                })
                .on_scroll(move |rect| {
                    scroll_pos.set(rect.origin());
                })
                .ensure_visible(move || ensure_visible.get())
                .on_resize(move |rect| {
                    settings_content_size.set(rect.size());
                })
                .style(|s| s.absolute().size_pct(100.0, 100.0))
            })
            .style(|s| s.size_pct(100.0, 100.0)),
        ))
        .style(|s| s.flex_col().size_pct(100.0, 100.0)),
    ))
    .style(|s| s.absolute().size_pct(100.0, 100.0))
    .debug_name("Settings")
    .on_cleanup(move || {
        editors.remove(search_editor_id);
    })
}
