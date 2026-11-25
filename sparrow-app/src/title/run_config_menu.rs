//! JetBrains-style run configuration chip + dropdown.
//!
//! The chip shows the active configuration; clicking it opens a dropdown with
//! the configurations grouped (run configs, then tests), a separator and
//! "Edit Configurations...". The highlighted row shows inline Run / Debug /
//! More buttons. Keyboard: Up/Down, Enter (select), Esc.
//!
//! Uses the same `add_overlay` mechanism as `widgets::popup_menu`, so no extra
//! signals or root-level views are required.

use floem::{
    AnyView, IntoView, View, ViewId,
    action::{add_overlay, remove_overlay},
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    kurbo::Point,
    peniko::Color,
    reactive::{
        RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_rw_signal,
    },
    style::CursorStyle,
    views::{Decorators, container, dyn_stack, empty, label, stack, svg},
};
use sparrow_rpc::dap_types::{DapId, RunDebugConfig};
use std::{path::Path, rc::Rc};

use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    debug::RunDebugMode,
    widgets::{
        divider::Divider,
        popup_menu::{Menu, MenuItem, show_context_menu},
        sparrow_scroll,
    },
    window_tab::WindowTabData,
};

const ROW_H: f32 = 22.0;

// ───────────────────────────── model ─────────────────────────────

#[derive(Clone)]
enum Item {
    Config {
        id: DapId,
        name: String,
        icon: &'static str,
    },
    Sep,
    Edit,
}

#[derive(Clone, Copy, PartialEq)]
enum Sel {
    Config(DapId),
    Edit,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ItemKey {
    Config(DapId),
    Separator(usize),
    Edit,
}

fn identity_index<T, Id: PartialEq>(
    items: &[T],
    selected: Id,
    identity: impl Fn(&T) -> Id,
) -> Option<usize> {
    items.iter().position(|item| identity(item) == selected)
}

fn set_active_config(wt: &WindowTabData, id: DapId) -> Option<usize> {
    let index = wt.run_configs.with_untracked(|configs| {
        identity_index(configs, id, |config| config.dap_id)
    });
    if let Some(index) = index {
        wt.active_run_config_idx.set(index);
    }
    index
}

fn config_icon(program: &str) -> &'static str {
    if Path::new(program)
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("flutter"))
    {
        SparrowIcons::FLUTTER
    } else {
        SparrowIcons::DART
    }
}

/// Configurations whose first argument is `test` are listed in their own group.
fn is_test(args: &Option<Vec<String>>) -> bool {
    args.as_ref()
        .and_then(|a| a.first())
        .is_some_and(|a| a == "test")
}

fn configuration_items(configs: &[RunDebugConfig]) -> (Vec<Item>, Vec<Sel>) {
    let (tests, runs): (Vec<_>, Vec<_>) = configs
        .iter()
        .enumerate()
        .partition(|(_, configuration)| is_test(&configuration.args));

    let mut items = Vec::new();
    let mut order = Vec::new();
    let mut add = |group: Vec<(DapId, String, &'static str)>| {
        if group.is_empty() {
            return;
        }
        if !items.is_empty() {
            items.push(Item::Sep);
        }
        for (id, name, icon) in group {
            items.push(Item::Config { id, name, icon });
            order.push(Sel::Config(id));
        }
    };
    add(runs
        .iter()
        .map(|(_, config)| {
            (
                config.dap_id,
                config.name.clone(),
                config_icon(&config.program),
            )
        })
        .collect());
    add(tests
        .iter()
        .map(|(_, config)| {
            (
                config.dap_id,
                config.name.clone(),
                config_icon(&config.program),
            )
        })
        .collect());
    if !items.is_empty() {
        items.push(Item::Sep);
    }
    items.push(Item::Edit);
    order.push(Sel::Edit);
    (items, order)
}

struct Dd {
    wt: Rc<WindowTabData>,
    open: RwSignal<bool>,
    overlay: ViewId,
    restore: Option<ViewId>,
    sel: RwSignal<Sel>,
    order: RwSignal<Vec<Sel>>,
}

fn dismiss(dd: &Dd) {
    remove_overlay(dd.overlay);
    dd.open.set(false);
    if let Some(r) = dd.restore {
        r.request_focus();
    }
}

fn activate(dd: &Dd, target: Sel, mode: Option<RunDebugMode>) {
    match target {
        Sel::Config(id) => {
            let idx = dd.wt.run_configs.with_untracked(|configs| {
                identity_index(configs, id, |config| config.dap_id)
            });
            dismiss(dd);
            if let Some(idx) = idx {
                dd.wt.active_run_config_idx.set(idx);
                if let Some(mode) = mode {
                    dd.wt.launch_selected_configuration(mode);
                }
            }
        }
        Sel::Edit => {
            dismiss(dd);
            dd.wt.run_config_modal_visible.set(true);
        }
    }
}

// ───────────────────────────── chip ─────────────────────────────

/// Drop-in replacement for the inline `run_config_chip` block in `actions.rs`.
pub(super) fn run_config_chip(window_tab_data: Rc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let open = create_rw_signal(false);
    let cursor = create_rw_signal(Sel::Edit);
    let items = create_rw_signal(Vec::<Item>::new());
    let order = create_rw_signal(Vec::<Sel>::new());
    let active_config = window_tab_data.active_run_config_idx;
    let configurations = window_tab_data.run_configs;
    let current_cursor = cursor;
    let current_items = items;
    let current_order = order;
    create_effect(move |_| {
        let configurations = configurations.get();
        let (items, order) = configuration_items(&configurations);
        current_items.set(items);
        current_order.set(order);
        let active = active_config.get();
        let selected = configurations
            .get(active)
            .map(|configuration| Sel::Config(configuration.dap_id));
        current_cursor.set(selected.unwrap_or(Sel::Edit));
    });

    let chip = container(
        stack((
            svg({
                let wt = window_tab_data.clone();
                move || {
                    let index = wt.active_run_config_idx.get();
                    let icon = wt.run_configs.with(|configs| {
                        configs
                            .get(index)
                            .map(|c| config_icon(&c.program))
                            .unwrap_or(SparrowIcons::SETTINGS)
                    });
                    config.get().ui_svg(icon)
                }
            })
            .style(|s| s.size(16.0, 16.0).margin_right(6.0)),
            label({
                let wt = window_tab_data.clone();
                move || {
                    let idx = wt.active_run_config_idx.get();
                    wt.run_configs.with(|c| {
                        c.get(idx)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| "Add Configuration...".to_string())
                    })
                }
            })
            .style(|s| s.selectable(false)),
            svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style(
                move |s| {
                    let size = config.get().ui.icon_size() as f32;
                    s.size(size, size).margin_left(4.0).color(
                        config.get().color(SparrowColor::SPARROW_ICON_INACTIVE),
                    )
                },
            ),
        ))
        .style(|s| s.items_center()),
    );
    let id = chip.id();

    chip.on_click_stop({
        let wt = window_tab_data.clone();
        move |_| {
            let r = id.layout_rect();
            show_dropdown(
                wt.clone(),
                Point::new(r.x0, r.y1 + 2.0),
                open,
                id,
                cursor,
                items,
                order,
            );
        }
    })
    .style(move |s| {
        let c = config.get();
        let is_open = open.get();
        s.height(28.0)
            .padding_horiz(10.0)
            .margin_horiz(6.0)
            .items_center()
            .border(1.0)
            .border_color(Color::TRANSPARENT)
            .border_radius(6.0)
            .apply_if(is_open, |s| {
                s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                    .border_color(c.color(SparrowColor::SPARROW_BORDER))
            })
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    })
}

// ───────────────────────────── dropdown ─────────────────────────────

fn show_dropdown(
    wt: Rc<WindowTabData>,
    anchor: Point,
    open: RwSignal<bool>,
    anchor_id: ViewId,
    cursor: RwSignal<Sel>,
    items: RwSignal<Vec<Item>>,
    order: RwSignal<Vec<Sel>>,
) {
    let config = wt.common.config;
    let window_size = wt.common.window_common.size;
    let restore = wt.common.keyboard_focus.get_untracked().or(Some(anchor_id));

    let active = wt.run_configs.with_untracked(|configs| {
        configs
            .get(wt.active_run_config_idx.get_untracked())
            .map(|configuration| configuration.dap_id)
    });
    let snapshot_items = items.get_untracked();
    let initial = active
        .map(Sel::Config)
        .filter(|selected| order.with_untracked(|order| order.contains(selected)))
        .unwrap_or(Sel::Edit);
    cursor.set(initial);

    let widest = snapshot_items
        .iter()
        .map(|i| match i {
            Item::Config { name, .. } => name.chars().count(),
            _ => 0,
        })
        .max()
        .unwrap_or(0) as f32;
    let width = (widest * 7.4 + 48.0 + 112.0).clamp(280.0, 480.0);

    let id = add_overlay(Point::ZERO, move |overlay| {
        open.set(true);
        let dd = Rc::new(Dd {
            wt: wt.clone(),
            open,
            overlay,
            restore,
            sel: cursor,
            order,
        });

        let rows = dyn_stack(
            {
                move || {
                    let mut separator_index = 0;
                    items
                        .get()
                        .into_iter()
                        .map(|item| {
                            let key = match &item {
                                Item::Config { id, .. } => ItemKey::Config(*id),
                                Item::Sep => {
                                    let key = ItemKey::Separator(separator_index);
                                    separator_index += 1;
                                    key
                                }
                                Item::Edit => ItemKey::Edit,
                            };
                            (key, item)
                        })
                        .collect::<Vec<_>>()
                }
            },
            |(key, _)| *key,
            {
                let dd = dd.clone();
                move |(_, item)| item_view(dd.clone(), item)
            },
        )
        .style(|s| s.flex_col().width_full());

        let bounds = window_size.get_untracked();
        let x = anchor
            .x
            .min((bounds.width - width as f64 - 8.0).max(0.0))
            .max(0.0);
        let max_h = (bounds.height - anchor.y - 16.0).max(120.0) as f32;

        let panel = sparrow_scroll(rows)
            .on_event_stop(EventListener::PointerDown, |_| {})
            .style(move |s| {
                let c = config.get();
                s.absolute()
                    .inset_left(x)
                    .inset_top(anchor.y)
                    .width(width)
                    .max_height(max_h)
                    .padding(8.0)
                    .border(1.0)
                    .border_radius(8.0)
                    .border_color(c.color(SparrowColor::SPARROW_BORDER))
                    .background(c.color(SparrowColor::PANEL_BACKGROUND))
                    .box_shadow_blur(14.0)
                    .box_shadow_v_offset(4.0)
                    .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
                    .font_size(13.0)
                    .font_family(c.ui.font_family.clone())
            });

        let view = stack((panel,))
            .keyboard_navigable()
            .on_event_stop(EventListener::PointerDown, {
                let dd = dd.clone();
                move |_| dismiss(&dd)
            })
            .on_event(EventListener::KeyDown, {
                let dd = dd.clone();
                move |event| {
                    let Event::KeyDown(key) = event else {
                        return EventPropagation::Continue;
                    };
                    let order = dd.order.get_untracked();
                    let n = order.len();
                    if n == 0 {
                        return EventPropagation::Continue;
                    }
                    let current = dd.sel.get_untracked();
                    let cur = order
                        .iter()
                        .position(|selection| *selection == current)
                        .unwrap_or(0);
                    match &key.key.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            crate::widgets::modal_focus::note_popup_escape();
                            dismiss(&dd);
                        }
                        Key::Named(NamedKey::Tab) => dismiss(&dd),
                        Key::Named(NamedKey::ArrowDown) => {
                            dd.sel.set(order[(cur + 1) % n]);
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            dd.sel.set(order[(cur + n - 1) % n]);
                        }
                        Key::Named(NamedKey::Home) => dd.sel.set(order[0]),
                        Key::Named(NamedKey::End) => dd.sel.set(order[n - 1]),
                        Key::Named(NamedKey::Enter | NamedKey::Space) => {
                            activate(&dd, current, None)
                        }
                        _ => {}
                    }
                    EventPropagation::Stop
                }
            })
            .on_event_stop(EventListener::WindowLostFocus, {
                let dd = dd.clone();
                move |_| {
                    // Don't steal focus back from the newly activated window.
                    remove_overlay(dd.overlay);
                    dd.open.set(false);
                }
            })
            .style(move |s| {
                let bounds = window_size.get();
                s.absolute().size(bounds.width, bounds.height).z_index(1000)
            });
        view.id().request_focus();
        view
    });
    let _ = id;
}

fn item_view(dd: Rc<Dd>, item: Item) -> AnyView {
    let config = dd.wt.common.config;
    match item {
        Item::Sep => Divider::new(config)
            .spacing(4.0)
            .indent(0.0)
            .into_view()
            .into_any(),
        Item::Edit => {
            let sel = dd.sel;
            let on_click = dd.clone();
            stack((
                empty().style(|s| s.width(24.0).flex_shrink(0.0_f32)),
                label(|| "Edit Configurations...".to_string()).style(move |s| {
                    s.color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
                        .selectable(false)
                }),
            ))
            .on_event_stop(EventListener::PointerEnter, move |_| sel.set(Sel::Edit))
            .on_click_stop(move |_| activate(&on_click, Sel::Edit, None))
            .style(move |s| row_style(config, s, move || sel.get() == Sel::Edit))
            .into_any()
        }
        Item::Config { id, name, icon } => {
            let sel = dd.sel;
            let selected = move || sel.get() == Sel::Config(id);

            let run_dd = dd.clone();
            let debug_dd = dd.clone();
            let more_dd = dd.clone();

            let button = move |inner: AnyView, on_click: Box<dyn Fn()>| -> AnyView {
                container(inner)
                    .on_click_stop(move |_| on_click())
                    .style(move |s| {
                        let c = config.get();
                        s.size(26.0, 20.0)
                            .items_center()
                            .justify_center()
                            .border_radius(4.0)
                            .hover(|s| {
                                s.cursor(CursorStyle::Pointer).background(
                                    c.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                                )
                            })
                    })
                    .into_any()
            };

            let run_btn = button(
                svg(move || config.get().ui_svg(SparrowIcons::START))
                    .style(move |s| {
                        s.size(16.0, 16.0).color(
                            config.get().color(SparrowColor::SPARROW_RUN_ICON),
                        )
                    })
                    .into_any(),
                Box::new(move || {
                    activate(&run_dd, Sel::Config(id), Some(RunDebugMode::Run))
                }),
            );
            let debug_btn = button(
                svg(move || config.get().ui_svg(SparrowIcons::DEBUG))
                    .style(move |s| {
                        s.size(16.0, 16.0).color(
                            config.get().color(SparrowColor::SPARROW_DEBUG_ICON),
                        )
                    })
                    .into_any(),
                Box::new(move || {
                    activate(&debug_dd, Sel::Config(id), Some(RunDebugMode::Debug))
                }),
            );
            let more_id: Rc<std::cell::Cell<Option<ViewId>>> = Rc::default();
            let more_holder = more_id.clone();
            let more_inner = label(|| "⋮".to_string()).style(move |s| {
                s.color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
                    .selectable(false)
            });
            more_holder.set(Some(more_inner.id()));
            let more_btn = button(
                more_inner.into_any(),
                Box::new(move || {
                    let at = more_id
                        .get()
                        .map(|id| {
                            let r = id.layout_rect();
                            Point::new(r.x0, r.y1 + 4.0)
                        })
                        .unwrap_or(Point::ZERO);
                    let wt = more_dd.wt.clone();
                    dismiss(&more_dd);
                    show_context_menu(config_menu(wt, id), Some(at));
                }),
            );

            let on_click = dd.clone();
            stack((
                svg(move || config.get().ui_svg(icon)).style(|s| {
                    s.size(16.0, 16.0).margin_right(8.0).flex_shrink(0.0_f32)
                }),
                label(move || name.clone()).style(move |s| {
                    s.flex_grow(1.0_f32)
                        .min_width(0.0)
                        .text_ellipsis()
                        .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
                        .selectable(false)
                }),
                stack((
                    empty().style(move |s| {
                        s.width(1.0).height(16.0).margin_horiz(6.0).background(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                    }),
                    run_btn,
                    debug_btn,
                    more_btn,
                ))
                .style(move |s| {
                    s.items_center().apply_if(!selected(), |s| s.hide())
                }),
            ))
            .on_event_stop(EventListener::PointerEnter, move |_| {
                sel.set(Sel::Config(id))
            })
            .on_click_stop(move |_| activate(&on_click, Sel::Config(id), None))
            .style(move |s| row_style(config, s, selected))
            .into_any()
        }
    }
}

fn config_menu(wt: Rc<WindowTabData>, id: DapId) -> Menu {
    let run = wt.clone();
    let debug = wt.clone();
    let edit = wt.clone();
    Menu::new("")
        .entry(MenuItem::new("Run").action(move || {
            if set_active_config(&run, id).is_some() {
                run.launch_selected_configuration(RunDebugMode::Run);
            }
        }))
        .entry(MenuItem::new("Debug").action(move || {
            if set_active_config(&debug, id).is_some() {
                debug.launch_selected_configuration(RunDebugMode::Debug);
            }
        }))
        .separator()
        .entry(MenuItem::new("Edit Configurations...").action(move || {
            if set_active_config(&edit, id).is_some() {
                edit.run_config_modal_visible.set(true);
            }
        }))
}

fn row_style(
    config: floem::reactive::ReadSignal<
        std::sync::Arc<crate::config::SparrowConfig>,
    >,
    s: floem::style::Style,
    selected: impl Fn() -> bool + 'static,
) -> floem::style::Style {
    let c = config.get();
    let bg = c
        .color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND)
        .multiply_alpha(0.55);
    s.width_full()
        .height(ROW_H)
        .flex_shrink(0.0_f32)
        .padding_horiz(8.0)
        .items_center()
        .border_radius(5.0)
        .apply_if(selected(), |s| s.background(bg))
        .hover(|s| s.cursor(CursorStyle::Pointer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_selection_uses_configuration_identity_after_reordering() {
        let first = DapId::next();
        let second = DapId::next();
        let configurations = [(first, "Duplicate"), (second, "Duplicate")];
        let reordered = [configurations[1], configurations[0]];

        assert_eq!(identity_index(&reordered, second, |(id, _)| *id), Some(0));
        assert_eq!(identity_index(&reordered, first, |(id, _)| *id), Some(1));
    }
}
