//! Logcat capture pane with device, query and scroll controls.

use crate::panel::logcat::{Buffer, Pane, State};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    widgets::popup_menu::{Menu, MenuItem, PopupMenuExt},
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    peniko::{Color, kurbo::Point},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    style::CursorStyle,
    views::{
        Decorators, dyn_container, empty, label, stack, svg, virtual_stack,
    },
};
use std::sync::Arc;
use crate::widgets::sparrow_scroll;
use super::history::history;
use super::row::{Row, log_row, wrap_message};
use super::toolbar::{
    PRIMARY_ACTIONS, TOOLBAR_BUTTON, TOOLBAR_DIVIDER, TOOLBAR_WIDTH, logcat_toolbar,
};

pub(super) fn pane_view(state: State, pane: Pane) -> impl View {
    let config = state.window.common.config;
    let width = create_rw_signal(900.0_f64);
    let devices = pane.clone();
    let chosen = pane.clone();
    let device = stack((
        svg(move || config.get().ui_svg(SparrowIcons::LOGCAT_DEVICE)).style(
            move |s| {
                s.size(14.0, 14.0)
                    .flex_shrink(0.0_f32)
                    .color(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE))
            },
        ),
        label(move || {
            if let Some(path) = devices.file_path.get() {
                return format!("Capture: {}", path.display());
            }
            let serial = devices.serial.get();
            devices.catalog.with(|c| {
                c.devices
                    .iter()
                    .find(|d| d.serial == serial)
                    .map(|d| d.label.clone())
                    .unwrap_or_else(|| "No connected Android device".into())
            })
        })
        .style(|s| s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()),
        svg(move || config.get().ui_svg(SparrowIcons::PALETTE_MENU)).style(
            move |s| {
                s.size(12.0, 12.0)
                    .flex_shrink(0.0_f32)
                    .color(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE))
            },
        ),
    ))
    .ide_popout_menu(move || {
        let mut menu = Menu::new("Android devices");
        for device in chosen.catalog.get_untracked().devices {
            let serial = chosen.serial;
            let file = chosen.file_path;
            let buffer = chosen.data;
            let ready = device.ready;
            menu = menu.entry(MenuItem::new(device.label).action(move || {
                if ready {
                    if file.get_untracked().is_some() {
                        buffer.update(Buffer::clear);
                        file.set(None);
                    }
                    serial.set(device.serial.clone())
                }
            }));
        }
        let restart = chosen.restart;
        menu.entry(
            MenuItem::new("Refresh devices")
                .action(move || restart.update(|r| *r += 1)),
        )
    })
    .style(move |s| {
        s.width(396.0)
            .min_width(0.0)
            .max_width_full()
            .flex_shrink(0.0_f32)
            .height(30.0)
            .items_center()
            .padding_horiz(12.0)
            .border(1.0)
            .border_radius(3.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .text_ellipsis()
            .cursor(CursorStyle::Pointer)
    });
    let hist = state.clone();
    let hist_p = pane.clone();
    let suggest = state.clone();
    let suggest_p = pane.clone();
    let record = state.clone();
    let q = pane.query;
    let case = pane.case;
    let clear = pane.query;
    let favorite = state.clone();
    let favorite_q = pane.query;
    let favorites = state.saved;
    let query=crate::widgets::text_input(pane.query,config).placeholder("Filter logs (Ctrl+Space)")
 .on_enter(move ||record.remember(&q.get_untracked(),false))
 .on_event(EventListener::KeyUp,move |event|{if let Event::KeyUp(key)=event{if key.modifiers.control()&&matches!(&key.key.logical_key,Key::Named(NamedKey::Space))||key.modifiers.control()&&matches!(&key.key.logical_key,Key::Character(c) if c.as_str()==" "){crate::widgets::popup_menu::show_context_menu(history(suggest.clone(),suggest_p.clone(),true),None);return EventPropagation::Stop;}}EventPropagation::Continue})
 .style(|s|s.flex_grow(1.0_f32).flex_basis(0).min_width(0.0).border(0.0).border_radius(0.0));
    let filter = stack((
        clickable_icon(
            || SparrowIcons::LOGCAT_FILTER,
            || {},
            || false,
            || false,
            || "Query history / Ctrl+Space for suggestions",
            config,
        )
        .ide_popout_menu(move || history(hist.clone(), hist_p.clone(), false)),
        query,
        clickable_icon(
            || SparrowIcons::CLOSE,
            move || clear.set(String::new()),
            || false,
            || false,
            || "Clear filter",
            config,
        ),
        label(|| "Cc")
            .on_click_stop(move |_| case.update(|c| *c = !*c))
            .style(move |s| {
                s.width(24.0)
                    .height(24.0)
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::Pointer)
                    .apply_if(case.get(), |s| {
                        s.background(
                            config
                                .get()
                                .color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                        )
                    })
            }),
        label(move || {
            if favorites.with(|s| s.favorites.contains(&favorite_q.get())) {
                "★"
            } else {
                "☆"
            }
        })
        .on_click_stop(move |_| favorite.remember(&favorite_q.get_untracked(), true))
        .style(|s| {
            s.size(24.0, 24.0)
                .items_center()
                .justify_center()
                .cursor(CursorStyle::Pointer)
        }),
    ))
    .style(move |s| {
        let narrow = width.get() <= 500.0;
        s.apply_if(!narrow, |s| s.flex_grow(1.0_f32).flex_basis(0))
            .apply_if(narrow, |s| s.width_full().flex_shrink(0.0_f32))
            .min_width(0.0)
            .height(30.0)
            .items_center()
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });
    let controls = stack((device, filter)).style(move |s| {
        s.width_full()
            .min_width(0.0)
            .height(if width.get() > 500.0 { 48.0 } else { 80.0 })
            .padding_horiz(10.0)
            .gap(12.0)
            .items_center()
            .flex_shrink(0.0_f32)
            .apply_if(width.get() <= 500.0, |s| {
                s.flex_col().items_start().padding_vert(5.0).gap(8.0)
            })
    });
    let error_p = pane.clone();
    let error_p2 = pane.clone();
    let packages = state.packages;
    let warning = label(move || {
        if let Err(error) = error_p.parsed.get() {
            format!("Invalid filter: {error}")
        } else {
            let error = error_p.error.get();
            if !error.is_empty() {
                error
            } else {
                error_p.catalog.with(|c| c.error.clone())
            }
        }
    })
    .style(move |s| {
        s.width_full()
            .padding_horiz(10.0)
            .padding_vert(4.0)
            .color(config.get().color(SparrowColor::SPARROW_ERROR))
            .apply_if(
                error_p2.parsed.with(|q| q.is_ok())
                    && error_p2.error.with(|e| e.is_empty())
                    && error_p2.catalog.with(|c| c.error.is_empty()),
                |s| s.hide(),
            )
    });
    let sync = state.clone();
    let mine_q = pane.query;
    let package_syncing = state.syncing;
    let package_warning = stack((
        label(|| "⚠ Select a build variant or sync Gradle to resolve package:mine.")
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()),
        label(|| "Sync now")
            .on_click_stop(move |_| sync.sync_gradle(true))
            .style(move |s| {
                s.color(config.get().color(SparrowColor::EDITOR_LINK))
                    .cursor(CursorStyle::Pointer)
            }),
    ))
    .style(move |s| {
        s.width_full()
            .min_width(0.0)
            .height(30.0)
            .flex_shrink(0.0_f32)
            .items_center()
            .gap(8.0)
            .padding_horiz(12.0)
            .background(Color::from_rgb8(69, 51, 29))
            .apply_if(
                package_syncing.get()
                    || !packages.with(|p| p.is_empty())
                    || !mine_q.with(|q| q.contains("package:mine")),
                |s| s.hide(),
            )
    });
    let rows_p = pane.clone();
    let rows_packages = state.packages;
    let shown = create_memo(move |_| {
        let now = rows_p.now.get();
        let Ok(query) = rows_p.parsed.get() else {
            return Vec::new();
        };
        let mine = rows_packages.get();
        let map = if rows_p.file_path.get().is_none() {
            rows_p.catalog.with(|c| c.processes.clone())
        } else {
            Default::default()
        };
        let entries = if rows_p.paused.get() {
            rows_p.frozen.get()
        } else {
            rows_p.data.with(|b| b.entries.iter().cloned().collect())
        };
        entries
            .into_iter()
            .filter_map(|entry| {
                let entry = if let Some((package, process)) = map.get(&entry.pid) {
                    if entry.package.is_empty() {
                        let mut value = (*entry).clone();
                        value.package = package.clone();
                        value.process = process.clone();
                        Arc::new(value)
                    } else {
                        entry
                    }
                } else {
                    entry
                };
                query.matches(&entry, &mine, now).then_some(entry)
            })
            .collect::<Vec<_>>()
    });
    if std::env::var_os("SPARROW_WAYLAND_GUARD").is_some() {
        let count = pane.visible_count;
        let packages = pane.visible_packages;
        floem::reactive::create_effect(move |_| {
            shown.with(|rows| {
                count.set(rows.len());
                packages.set(
                    rows.iter()
                        .map(|e| e.package.clone())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                );
            });
        });
    }
    let follow = pane.follow;
    let jump = create_rw_signal(None::<usize>);
    let body_height = create_rw_signal(0.0_f64);
    let toolbar_state = state.clone();
    let toolbar_pane = pane.clone();
    // Reserve room for the chevron and its separator.
    let primary_count = create_memo(move |_| {
        (((body_height.get() - TOOLBAR_DIVIDER).max(0.0) / TOOLBAR_BUTTON).floor()
            as usize)
            .saturating_sub(1)
            .min(PRIMARY_ACTIONS)
    });
    let toolbar = dyn_container(
        move || primary_count.get(),
        move |count| {
            logcat_toolbar(
                toolbar_state.clone(),
                toolbar_pane.clone(),
                shown,
                jump,
                count,
            )
        },
    );
    let formatting = pane.format;
    let wrap = pane.wrap;
    let row_height = move || formatting.get().height();
    let content_width = create_rw_signal(1200.0_f64);
    let visual = create_memo(move |_| {
        let format = formatting.get();
        let wrap = wrap.get();
        let chars = ((content_width.get() - format.metadata_width() - 10.0)
            / (format.font_size as f64 * 0.62))
            .max(1.0) as usize;
        shown
            .get()
            .into_iter()
            .flat_map(|entry| {
                let parts = if wrap {
                    wrap_message(&entry.message, chars)
                } else {
                    vec![entry.message.replace(['\n', '\r'], " ")]
                };
                let format = format.clone();
                parts
                    .into_iter()
                    .enumerate()
                    .map(move |(index, message)| Row {
                        entry: entry.clone(),
                        index,
                        message,
                        format: format.clone(),
                        wrap,
                    })
            })
            .collect::<Vec<_>>()
    });
    let row_window = state.window.clone();
    let rows = virtual_stack(
        move || visual.get().into_iter().collect::<im::Vector<_>>(),
        |row| {
            (
                row.entry.id,
                row.index,
                row.message.clone(),
                row.format.clone(),
                row.wrap,
                row.entry.package.clone(),
                row.entry.process.clone(),
            )
        },
        move |row| log_row(row, row_window.clone()),
    )
    .item_size_fixed(move || row_height())
    .style(|s| s.flex_col().min_width_full());
    let viewport = sparrow_scroll(rows)
        .on_event_cont(EventListener::PointerWheel, move |_| {
            follow.set(false);
            jump.set(None);
        })
        .on_click_stop(move |_| follow.set(false))
        .scroll_to(move || {
            if follow.get() {
                let count = visual.with(|v| v.len());
                Some(Point::new(0.0, count as f64 * row_height()))
            } else {
                jump.get()
                    .and_then(|index| {
                        shown.with(|rows| rows.get(index).map(|e| e.id))
                    })
                    .and_then(|id| {
                        visual
                            .with(|rows| rows.iter().position(|r| r.entry.id == id))
                    })
                    .map(|index| Point::new(0.0, index as f64 * row_height()))
            }
        })
        .on_resize(move |rect| content_width.set(rect.width()))
        .style(|s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0)
                .min_width(0.0)
                .min_height(0.0)
                .width_full()
        });
    stack((
        controls,
        warning,
        stack((
            empty().style(|s| s.width(TOOLBAR_WIDTH).flex_shrink(0.0_f32)),
            stack((package_warning, viewport)).style(|s| {
                s.flex_col()
                    .flex_grow(1.0_f32)
                    .flex_basis(0)
                    .min_width(0.0)
                    .min_height(0.0)
                    .height_full()
            }),
            toolbar.style(|s| {
                s.absolute()
                    .inset_left(0.0)
                    .inset_top(0.0)
                    .size_full()
                    .z_index(10)
            }),
        ))
        .on_resize(move |rect| body_height.set(rect.height()))
        .style(|s| {
            s.width_full()
                .flex_grow(1.0_f32)
                .flex_basis(0)
                .min_height(0.0)
        }),
    ))
    .on_resize(move |rect| width.set(rect.width()))
    .style(move |s| {
        s.flex_col()
            .size_full()
            .min_width(0.0)
            .font_size(13.0)
            .color(config.get().color(SparrowColor::PANEL_FOREGROUND))
    })
}
