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
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    style::CursorStyle,
    views::{Decorators, container, dyn_stack, empty, label, stack, svg},
};
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
        idx: usize,
        pos: usize,
        name: String,
        icon: &'static str,
    },
    Sep,
    Edit {
        pos: usize,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum Sel {
    Config(usize),
    Edit,
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

struct Dd {
    wt: Rc<WindowTabData>,
    open: RwSignal<bool>,
    overlay: ViewId,
    restore: Option<ViewId>,
    sel: RwSignal<usize>,
    order: Vec<Sel>,
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
        Sel::Config(idx) => {
            dd.wt.active_run_config_idx.set(idx);
            dismiss(dd);
            if let Some(mode) = mode {
                dd.wt.launch_selected_configuration(mode);
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
                    s.size(size, size)
                        .margin_left(4.0)
                        .color(config.get().color(SparrowColor::SPARROW_ICON_INACTIVE))
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
            show_dropdown(wt.clone(), Point::new(r.x0, r.y1 + 2.0), open, id);
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
) {
    let config = wt.common.config;
    let window_size = wt.common.window_common.size;
    let restore = wt.common.keyboard_focus.get_untracked().or(Some(anchor_id));

    // Snapshot the configurations in display order.
    let configs = wt.run_configs.get_untracked();
    let active = wt.active_run_config_idx.get_untracked();
    let (tests, runs): (Vec<_>, Vec<_>) = configs
        .iter()
        .enumerate()
        .partition(|(_, c)| is_test(&c.args));

    let mut items: Vec<Item> = Vec::new();
    let mut order: Vec<Sel> = Vec::new();
    let mut add = |items: &mut Vec<Item>, group: Vec<(usize, String, &'static str)>| {
        if group.is_empty() {
            return;
        }
        if !items.is_empty() {
            items.push(Item::Sep);
        }
        for (idx, name, icon) in group {
            items.push(Item::Config {
                idx,
                pos: order.len(),
                name,
                icon,
            });
            order.push(Sel::Config(idx));
        }
    };
    add(
        &mut items,
        runs.iter()
            .map(|(i, c)| (*i, c.name.clone(), config_icon(&c.program)))
            .collect(),
    );
    add(
        &mut items,
        tests
            .iter()
            .map(|(i, c)| (*i, c.name.clone(), config_icon(&c.program)))
            .collect(),
    );
    if !items.is_empty() {
        items.push(Item::Sep);
    }
    items.push(Item::Edit { pos: order.len() });
    order.push(Sel::Edit);

    let initial = order
        .iter()
        .position(|s| *s == Sel::Config(active))
        .unwrap_or(0);

    let widest = items
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
            sel: create_rw_signal(initial),
            order: order.clone(),
        });

        let rows = dyn_stack(
            {
                let items = items.clone();
                move || items.clone().into_iter().enumerate().collect::<Vec<_>>()
            },
            |(i, _)| *i,
            {
                let dd = dd.clone();
                move |(_, item)| item_view(dd.clone(), item)
            },
        )
            .style(|s| s.flex_col().width_full());

        let bounds = window_size.get_untracked();
        let x = anchor.x.min((bounds.width - width as f64 - 8.0).max(0.0)).max(0.0);
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
                    let n = dd.order.len();
                    let cur = dd.sel.get_untracked();
                    match &key.key.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            crate::widgets::modal_focus::note_popup_escape();
                            dismiss(&dd);
                        }
                        Key::Named(NamedKey::Tab) => dismiss(&dd),
                        Key::Named(NamedKey::ArrowDown) => dd.sel.set((cur + 1) % n),
                        Key::Named(NamedKey::ArrowUp) => dd.sel.set((cur + n - 1) % n),
                        Key::Named(NamedKey::Home) => dd.sel.set(0),
                        Key::Named(NamedKey::End) => dd.sel.set(n - 1),
                        Key::Named(NamedKey::Enter | NamedKey::Space) => {
                            activate(&dd, dd.order[cur], None)
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
        Item::Edit { pos } => {
            let sel = dd.sel;
            let on_click = dd.clone();
            stack((
                empty().style(|s| s.width(24.0).flex_shrink(0.0_f32)),
                label(|| "Edit Configurations...".to_string()).style(move |s| {
                    s.color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
                        .selectable(false)
                }),
            ))
                .on_event_stop(EventListener::PointerEnter, move |_| sel.set(pos))
                .on_click_stop(move |_| activate(&on_click, Sel::Edit, None))
                .style(move |s| row_style(config, s, move || sel.get() == pos))
                .into_any()
        }
        Item::Config {
            idx,
            pos,
            name,
            icon,
        } => {
            let sel = dd.sel;
            let selected = move || sel.get() == pos;

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
                        s.size(16.0, 16.0)
                            .color(config.get().color(SparrowColor::SPARROW_RUN_ICON))
                    })
                    .into_any(),
                Box::new(move || {
                    activate(&run_dd, Sel::Config(idx), Some(RunDebugMode::Run))
                }),
            );
            let debug_btn = button(
                svg(move || config.get().ui_svg(SparrowIcons::DEBUG))
                    .style(move |s| {
                        s.size(16.0, 16.0)
                            .color(config.get().color(SparrowColor::SPARROW_DEBUG_ICON))
                    })
                    .into_any(),
                Box::new(move || {
                    activate(&debug_dd, Sel::Config(idx), Some(RunDebugMode::Debug))
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
                    show_context_menu(config_menu(wt, idx), Some(at));
                }),
            );

            let on_click = dd.clone();
            stack((
                svg(move || config.get().ui_svg(icon))
                    .style(|s| s.size(16.0, 16.0).margin_right(8.0).flex_shrink(0.0_f32)),
                label(move || name.clone()).style(move |s| {
                    s.flex_grow(1.0_f32)
                        .min_width(0.0)
                        .text_ellipsis()
                        .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
                        .selectable(false)
                }),
                stack((
                    empty().style(move |s| {
                        s.width(1.0)
                            .height(16.0)
                            .margin_horiz(6.0)
                            .background(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                    }),
                    run_btn,
                    debug_btn,
                    more_btn,
                ))
                    .style(move |s| s.items_center().apply_if(!selected(), |s| s.hide())),
            ))
                .on_event_stop(EventListener::PointerEnter, move |_| sel.set(pos))
                .on_click_stop(move |_| {
                    activate(&on_click, Sel::Config(idx), None)
                })
                .style(move |s| row_style(config, s, selected))
                .into_any()
        }
    }
}

fn config_menu(wt: Rc<WindowTabData>, idx: usize) -> Menu {
    let run = wt.clone();
    let debug = wt.clone();
    let edit = wt.clone();
    Menu::new("")
        .entry(MenuItem::new("Run").action(move || {
            run.active_run_config_idx.set(idx);
            run.launch_selected_configuration(RunDebugMode::Run);
        }))
        .entry(MenuItem::new("Debug").action(move || {
            debug.active_run_config_idx.set(idx);
            debug.launch_selected_configuration(RunDebugMode::Debug);
        }))
        .separator()
        .entry(MenuItem::new("Edit Configurations...").action(move || {
            edit.active_run_config_idx.set(idx);
            edit.run_config_modal_visible.set(true);
        }))
}

fn row_style(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::SparrowConfig>>,
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