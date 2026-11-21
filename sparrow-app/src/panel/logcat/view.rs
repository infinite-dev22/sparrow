use super::{
    Buffer, Pane, State,
    model::{self, Entry},
    query::Query,
};
use crate::divider::{Divider, DividerVariant};
use crate::panel::kind::PanelKind;
use crate::tooltip::TooltipExt;
use crate::{
    app::clickable_icon,
    config::{color::SparrowColor, icon::SparrowIcons},
    popup_menu::{Menu, MenuItem, PopupMenuExt},
};
use floem::{
    IntoView, View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    peniko::{Color, kurbo::Point},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    style::CursorStyle,
    views::{
        Decorators, dyn_container, dyn_stack, empty, label, scroll, stack, svg,
        virtual_stack,
    },
};
use std::sync::Arc;

pub(super) fn panel(state: State) -> impl View {
    let config = state.window.common.config;
    let observed = state.clone();
    crate::ui_test_guard::observe_logcat(move || {
        serde_json::json!({
            "active":observed.active.get(),"split":observed.split.get(),
            "run_configuration":observed.window.run_configs.with(|c|c.get(observed.window.active_run_config_idx.get()).map(|c|c.name.clone())),
            "packages":observed.packages.get(),"variant":observed.variant.get(),"variants":observed.model.with(|m|m.variants.clone()),"gradle_syncing":observed.syncing.get(),"gradle_error":observed.gradle_error.get(),
            "panes":observed.panes.with(|panes|panes.iter().map(|p|serde_json::json!({
                "id":p.id,"query":p.query.get(),"serial":p.serial.get(),"paused":p.paused.get(),
                "follow":p.follow.get(),"wrap":p.wrap.get(),"compact":p.compact.get(),
                "records":p.data.with(|b|b.entries.len()),"snapshot_records":p.frozen.with(|r|r.len()),"last_id":p.data.with(|b|b.entries.back().map(|e|e.id)),"snapshot_last_id":p.frozen.with(|r|r.last().map(|e|e.id)),
                "error":p.error.get(),"query_valid":p.parsed.with(|q|q.is_ok()),"visible_count":p.visible_count.get(),"visible_packages":p.visible_packages.get(),
                "file":p.file_path.get(),"loading":p.loading.get(),"format":p.format.get(),"markers":p.data.with(|b|b.entries.iter().filter(|e|e.marker).map(|e|e.message.clone()).collect::<Vec<_>>()),
                "devices":p.catalog.with(|c|c.devices.iter().map(|d|serde_json::json!({"serial":d.serial,"ready":d.ready})).collect::<Vec<_>>())
            })).collect::<Vec<_>>())
        })
    });

    let tabs_state = state.clone();
    let add = state.clone();
    let open_capture = state.clone();
    let cleanup = state.clone();
    let tabs = scroll(
        dyn_stack(move || tabs_state.panes.get(), |p| p.id, {
            let state = state.clone();
            move |pane: Pane| {
                let id = pane.id;
                let close = state.clone();
                let active = state.active;
                stack((
                    label(move || {
                        if let Some(path) = pane.file_path.get() {
                            return path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                        }
                        if id == 0 {
                            "Logcat".into()
                        } else {
                            format!("Logcat {}", id + 1)
                        }
                    })
                    .style(|s| s.text_ellipsis().max_width(160.0)),
                    clickable_icon(
                        || SparrowIcons::CLOSE,
                        move || close.remove(id),
                        || false,
                        || false,
                        || "Close Logcat tab",
                        config,
                    )
                    .style(|s| s.size(18.0, 18.0).padding(3.0)),
                ))
                .on_click_stop(move |_| active.set(id))
                .style(move |s| {
                    s.height(24.0)
                        .items_center()
                        .padding_horiz(7.0)
                        .margin_right(4.0)
                        .flex_shrink(0.0_f32)
                        .border_radius(3.0)
                        .border(1.0)
                        .border_color(if active.get() == id {
                            config.get().color(SparrowColor::EDITOR_LINK)
                        } else {
                            Color::TRANSPARENT
                        })
                        .apply_if(active.get() == id, |s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                            )
                        })
                        .cursor(CursorStyle::Pointer)
                })
            }
        })
        .style(|s| s.items_center()),
    )
    .scroll_style(|s| s.hide_bars(true).vertical_scroll_as_horizontal(true))
    .style(|s| s.min_width(0.0).flex_shrink(1.0_f32));
    let header = super::super::chrome::header(
        state.window.clone(),
        PanelKind::Logcat,
        "Logcat",
        stack((
            tabs,
            clickable_icon(
                || SparrowIcons::ADD,
                move || add.add(),
                || false,
                || false,
                || "New Logcat tab",
                config,
            ),
            clickable_icon(
                || SparrowIcons::DIRECTORY,
                move || open_capture.show_import.set(true),
                || false,
                || false,
                || "Open capture file…",
                config,
            ),
        ))
        .style(|s| s.items_center().min_width(0.0).flex_shrink(1.0_f32)),
    );
    let import_state = state.clone();
    let load = state.clone();
    let browse = state.clone();
    let cancel = state.show_import;
    let import_bar = stack((
        crate::text_input::text_input(state.import_path, config)
            .placeholder("Capture path (.log, .txt, .json or bugreport .zip)")
            .on_enter(move || {
                let path = import_state.import_path.get_untracked();
                if !path.trim().is_empty() {
                    import_state.import(std::path::PathBuf::from(path.trim()));
                }
            })
            .style(|s| s.flex_grow(1.0_f32).flex_basis(0).min_width(0.0)),
        label(|| "Open")
            .on_click_stop(move |_| {
                let path = load.import_path.get_untracked();
                if !path.trim().is_empty() {
                    load.import(std::path::PathBuf::from(path.trim()));
                }
            })
            .style(|s| s.padding_horiz(8.0).cursor(CursorStyle::Pointer)),
        label(|| "Browse…")
            .on_click_stop(move |_| {
                let state = browse.clone();
                floem::action::open_file(
                    floem::file::FileDialogOptions::new()
                        .title("Open Logcat capture"),
                    move |file| {
                        if let Some(mut file) = file {
                            if let Some(path) = file.path.pop() {
                                state.import(path);
                            }
                        }
                    },
                );
            })
            .style(|s| s.padding_horiz(8.0).cursor(CursorStyle::Pointer)),
        clickable_icon(
            || SparrowIcons::CLOSE,
            move || cancel.set(false),
            || false,
            || false,
            || "Close capture path",
            config,
        ),
    ))
    .style(move |s| {
        s.width_full()
            .height(38.0)
            .items_center()
            .gap(6.0)
            .padding_horiz(10.0)
            .flex_shrink(0.0_f32)
            .apply_if(!state.show_import.get(), |s| s.hide())
    });
    let current = state.clone();
    let body_state = state.clone();
    let body = dyn_container(
        move || {
            (
                current.active.get(),
                current.split.get(),
                current
                    .panes
                    .with(|p| p.iter().map(|p| p.id).collect::<Vec<_>>()),
            )
        },
        move |(active, split, _)| {
            let panes = body_state.panes.get_untracked();
            let Some(pane) = panes.iter().find(|p| p.id == active).cloned() else {
                return label(|| "Use + to open a Logcat tab.")
                    .style(|s| s.padding(16.0))
                    .into_any();
            };
            if split {
                if let Some(other) = panes.into_iter().find(|p| p.id != active) {
                    return stack((
                        pane_view(body_state.clone(), pane).style(|s| {
                            s.width_pct(50.0).min_width(0.0).height_full()
                        }),
                        Divider::new(config)
                            .variant(DividerVariant::Vertical)
                            .into_view(),
                        pane_view(body_state.clone(), other).style(|s| {
                            s.flex_grow(1.0_f32)
                                .flex_basis(0)
                                .min_width(0.0)
                                .height_full()
                        }),
                    ))
                    .style(|s| s.size_full())
                    .into_any();
                }
            }
            pane_view(body_state.clone(), pane).into_any()
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0)
            .min_height(0.0)
            .width_full()
    });
    let variants = variant_bar(state.clone());
    stack((header, import_bar, variants, body))
        .on_cleanup(move || {
            cleanup
                .sync_cancel
                .store(true, std::sync::atomic::Ordering::Release);
            cleanup.save();
            for pane in cleanup.panes.get_untracked() {
                pane.close();
            }
        })
        .style(|s| s.size_full().flex_col().min_width(0.0))
        .debug_name("Logcat")
}
fn history(state: State, pane: Pane, suggestions: bool) -> Menu {
    let mut menu = Menu::new(if suggestions {
        "Logcat query suggestions"
    } else {
        "Filter history"
    });
    let mut filters = state.saved.get_untracked().favorites;
    filters.extend(state.saved.get_untracked().history);
    if suggestions {
        filters.extend(
            [
                "package:mine",
                "tag:",
                "-tag:",
                "message:",
                "level:ERROR",
                "level:WARN",
                "age:5m",
                "is:crash",
                "is:stacktrace",
            ]
            .map(String::from),
        );
        filters.extend(pane.data.with_untracked(|b| {
            b.entries
                .iter()
                .rev()
                .take(100)
                .map(|e| format!("tag:{}", e.tag))
                .collect::<Vec<_>>()
        }));
    }
    let mut seen = std::collections::HashSet::new();
    filters.retain(|q| seen.insert(q.clone()));
    for query in filters.into_iter().take(50) {
        let p = pane.clone();
        let remember = state.clone();
        let title = query.clone();
        let count = Query::parse(&query, pane.case.get_untracked())
            .ok()
            .map(|q| {
                pane.data.with_untracked(|b| {
                    b.entries
                        .iter()
                        .filter(|e| {
                            q.matches(
                                e,
                                &state.packages.get_untracked(),
                                chrono::Local::now().timestamp(),
                            )
                        })
                        .count()
                })
            })
            .unwrap_or(0);
        menu = menu.entry(MenuItem::new(format!("{title}    {count}")).action(
            move || {
                p.query.set(query.clone());
                remember.remember(&query, false);
            },
        ));
    }
    let remove = state.clone();
    menu.entry(MenuItem::new("Clear filter history").action(move || {
        remove.saved.update(|s| s.history.clear());
        remove.save();
    }))
}
#[derive(Clone)]
struct VariantChoice(String);
impl std::fmt::Display for VariantChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            f.write_str("Follow run configuration")
        } else {
            f.write_str(&self.0)
        }
    }
}
fn variant_bar(state: State) -> impl View {
    let config = state.window.common.config;
    let model = state.model;
    let choices_state = state.clone();
    let choices = dyn_container(
        move || model.get().variants,
        move |variants| {
            let current = create_rw_signal(VariantChoice(
                choices_state.variant.get_untracked(),
            ));
            let mut items = vec![VariantChoice(String::new())];
            items.extend(variants.into_iter().map(|v| VariantChoice(v.key())));
            let state = choices_state.clone();
            crate::dropdown::dropdown(current, items, config)
                .on_accept(move |choice| {
                    current.set(choice.clone());
                    state.variant.set(choice.0);
                    state.save();
                })
                .style(|s| s.width(280.0).max_width_full().flex_shrink(1.0_f32))
        },
    );
    let choices = choices.style(|s| s.min_width(0.0).flex_shrink(1.0_f32));
    let summary = state.clone();
    let details = state.clone();
    let refresh = state.clone();
    let busy = state.syncing;
    let exists = state
        .window
        .workspace
        .path
        .as_deref()
        .and_then(super::gradle::build_root)
        .is_some();
    stack((
        label(|| "Variant:").style(|s| s.flex_shrink(0.0_f32)),
        choices,
        label(move || {
            if summary.syncing.get() {
                return "Syncing Gradle variants…".into();
            }
            let error = summary.gradle_error.get();
            if !error.is_empty() {
                return error.lines().next().unwrap_or("Gradle sync failed").into();
            }
            let packages = summary.packages.get();
            if packages.is_empty() {
                "No matching variant; select a variant or sync Gradle".into()
            } else {
                packages.join(", ")
            }
        })
        .style(|s| s.text_ellipsis().min_width(0.0).flex_grow(1.0_f32))
        .tooltip(move || {
            let error = details.gradle_error.get();
            scroll(label(move || {
                if error.is_empty() {
                    details.packages.get().join(", ")
                } else {
                    error.clone()
                }
            }))
            .style(|s| s.max_width(650.0).max_height(300.0))
        }),
        clickable_icon(
            || SparrowIcons::LOGCAT_REFRESH,
            move || refresh.sync_gradle(true),
            || false,
            move || busy.get(),
            || "Sync Gradle variants",
            config,
        ),
    ))
    .style(move |s| {
        s.width_full()
            .min_width(0.0)
            .height(36.0)
            .items_center()
            .gap(8.0)
            .padding_horiz(10.0)
            .flex_shrink(0.0_f32)
            .apply_if(!exists, |s| s.hide())
    })
}
fn formatting_menu(pane: Pane, state: State) -> Menu {
    let mut menu = Menu::new("Logcat formatting");
    for (title, compact) in [("Standard", false), ("Compact", true)] {
        let pane = pane.clone();
        let state = state.clone();
        menu = menu.entry(MenuItem::new(title).action(move || {
            pane.compact.set(compact);
            pane.format.set(if compact {
                super::format::Formatting::compact()
            } else {
                super::format::Formatting::default()
            });
            state.save();
        }));
    }
    let current = pane.format.get_untracked();
    let mut columns = Menu::new("Visible columns");
    for (index, title, enabled) in [
        (0, "Timestamp", current.timestamp),
        (1, "Date", current.date),
        (2, "Milliseconds", current.milliseconds),
        (3, "Process ID", current.pid),
        (4, "Thread ID", current.tid),
        (5, "Tag", current.tag),
        (6, "Package name", current.package),
        (7, "Process name", current.process),
        (8, "Severity", current.level),
    ] {
        let pane = pane.clone();
        let state = state.clone();
        columns = columns.entry(
            MenuItem::new(format!("{}{}", if enabled { "✓ " } else { "" }, title))
                .action(move || {
                    pane.compact.set(false);
                    pane.format.update(|f| {
                        let value = match index {
                            0 => &mut f.timestamp,
                            1 => &mut f.date,
                            2 => &mut f.milliseconds,
                            3 => &mut f.pid,
                            4 => &mut f.tid,
                            5 => &mut f.tag,
                            6 => &mut f.package,
                            7 => &mut f.process,
                            _ => &mut f.level,
                        };
                        *value = !*value;
                    });
                    state.save();
                }),
        );
    }
    menu = menu.entry(columns);
    for (title, values, field) in [
        ("Tag width", vec![120, 192, 280, 400], 0),
        ("Package width", vec![160, 288, 420, 600], 1),
        ("Font size", vec![10, 11, 12, 13, 14, 16, 18, 20, 24], 2),
    ] {
        let mut options = Menu::new(title);
        for value in values {
            let pane = pane.clone();
            let state = state.clone();
            options =
                options.entry(MenuItem::new(value.to_string()).action(move || {
                    pane.format.update(|f| match field {
                        0 => f.tag_width = value,
                        1 => f.package_width = value,
                        _ => f.font_size = value,
                    });
                    state.save();
                }));
        }
        menu = menu.entry(options);
    }
    menu
}
fn pane_view(state: State, pane: Pane) -> impl View {
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
    let query=crate::text_input::text_input(pane.query,config).placeholder("Filter logs (Ctrl+Space)")
 .on_enter(move ||record.remember(&q.get_untracked(),false))
 .on_event(EventListener::KeyUp,move |event|{if let Event::KeyUp(key)=event{if key.modifiers.control()&&matches!(&key.key.logical_key,Key::Named(NamedKey::Space))||key.modifiers.control()&&matches!(&key.key.logical_key,Key::Character(c) if c.as_str()==" "){crate::popup_menu::show_context_menu(history(suggest.clone(),suggest_p.clone(),true),None);return EventPropagation::Stop;}}EventPropagation::Continue})
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
    let viewport = scroll(rows)
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
fn toolbar_actions(
    state: State,
    pane: Pane,
    shown: floem::reactive::Memo<Vec<Arc<Entry>>>,
    jump: floem::reactive::RwSignal<Option<usize>>,
) -> Vec<floem::AnyView> {
    let config = state.window.common.config;
    let pause = pane.clone();
    let clear_p = pane.clone();
    let restart_pane = pane.clone();
    let restart_window = state.window.clone();
    let follow = pane.follow;
    let compact = pane.compact;
    let format_pane = pane.clone();
    let format_state = state.clone();
    let wrap = pane.wrap;
    let split = state.clone();
    let next_rows = shown;
    let previous_rows = shown;
    let copy = shown;
    let export = shown;
    let export_window = state.window.clone();
    vec![
        clickable_icon(
            || SparrowIcons::LOGCAT_CLEAR,
            move || {
                clear_p.data.update(Buffer::clear);
                clear_p.frozen.set(Vec::new());
            },
            || false,
            || false,
            || "Clear displayed logs",
            config,
        )
        .into_any(),
        clickable_icon(
            move || {
                if pane.paused.get() {
                    SparrowIcons::DEBUG_CONTINUE
                } else {
                    SparrowIcons::DEBUG_PAUSE
                }
            },
            move || pause.pause(),
            || false,
            || false,
            || "Pause / Resume display",
            config,
        )
        .into_any(),
        clickable_icon(
            || SparrowIcons::LOGCAT_REFRESH,
            move || {
                if let Some(path) = restart_pane.file_path.get_untracked() {
                    restart_pane.load_file(
                        path,
                        restart_window.clone(),
                        Some(restart_pane.query.get_untracked()),
                    );
                } else {
                    restart_pane.restart.update(|r| *r += 1);
                }
            },
            || false,
            || false,
            || "Restart Logcat",
            config,
        )
        .into_any(),
        clickable_icon(
            || SparrowIcons::FOLD_DOWN,
            move || follow.set(true),
            move || follow.get(),
            || false,
            || "Scroll to end",
            config,
        )
        .into_any(),
        clickable_icon(
            || SparrowIcons::SEARCH_BACKWARD,
            move || {
                follow.set(false);
                let rows = previous_rows.get_untracked();
                let current = jump.get_untracked().unwrap_or(rows.len());
                jump.set(
                    rows.iter()
                        .enumerate()
                        .rev()
                        .find(|(i, e)| {
                            *i < current
                                && model::priority(&e.level.to_string()).unwrap_or(0)
                                    >= 4
                        })
                        .map(|(i, _)| i),
                );
            },
            || false,
            || false,
            || "Previous error",
            config,
        )
        .into_any(),
        clickable_icon(
            || SparrowIcons::SEARCH_FORWARD,
            move || {
                follow.set(false);
                let current = jump.get_untracked();
                jump.set(next_rows.with_untracked(|rows| {
                    rows.iter()
                        .enumerate()
                        .find(|(i, e)| {
                            current.is_none_or(|current| *i > current)
                                && model::priority(&e.level.to_string()).unwrap_or(0)
                                    >= 4
                        })
                        .map(|(i, _)| i)
                }));
            },
            || false,
            || false,
            || "Next error",
            config,
        )
        .into_any(),
        clickable_icon(
            || SparrowIcons::LOGCAT_WRAP,
            move || wrap.update(|w| *w = !*w),
            move || wrap.get(),
            || false,
            || "Soft wrap messages",
            config,
        )
        .into_any(),
        clickable_icon(
            || SparrowIcons::SETTINGS,
            || {},
            move || compact.get(),
            || false,
            || "Configure Logcat formatting",
            config,
        )
        .ide_popout_menu(move || {
            formatting_menu(format_pane.clone(), format_state.clone())
        })
        .into_any(),
        clickable_icon(
            || SparrowIcons::SPLIT_HORIZONTAL,
            move || {
                if split.panes.with_untracked(|p| p.len()) < 2 {
                    split.add();
                }
                split.split.update(|s| *s = !*s);
            },
            move || state.split.get(),
            || false,
            || "Split / Unsplit Logcat",
            config,
        )
        .into_any(),
        clickable_icon(
            || SparrowIcons::LOGCAT_COPY,
            move || {
                crate::menus::copy_text(
                    &copy
                        .get_untracked()
                        .iter()
                        .map(|e| e.raw.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            },
            || false,
            || false,
            || "Copy filtered logs",
            config,
        )
        .into_any(),
        clickable_icon(
            || SparrowIcons::LOGCAT_EXPORT,
            move || {
                let entries = export.get_untracked();
                let window = export_window.clone();
                floem::action::save_as(
                    floem::file::FileDialogOptions::new().title("Export Logcat"),
                    move |file| {
                        if let Some(mut file) = file {
                            if let Some(path) = file.path.pop() {
                                let content = if path
                                    .extension()
                                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
                                {
                                    super::archive::encode(&entries)
                                } else {
                                    Ok(entries
                                        .iter()
                                        .map(|e| e.raw.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n"))
                                };
                                if let Err(error) = content.and_then(|text| {
                                    std::fs::write(path, text)
                                        .map_err(|e| e.to_string())
                                }) {
                                    window.show_alert(
                                        "Cannot export logs".into(),
                                        error.to_string(),
                                        Vec::new(),
                                    );
                                }
                            }
                        }
                    },
                );
            },
            || false,
            || false,
            || "Export filtered logs…",
            config,
        )
        .into_any(),
        label(|| "?")
            .on_click_stop(move |_| {
                let _ =
                    open::that("https://developer.android.com/studio/debug/logcat");
            })
            .style(|s| {
                s.size(24.0, 24.0)
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::Pointer)
            })
            .into_any(),
    ]
}

const TOOLBAR_BUTTON: f64 = 26.0;
const TOOLBAR_WIDTH: f64 = 34.0;
const TOOLBAR_DIVIDER: f64 = 1.0;
const PRIMARY_ACTIONS: usize = 7;

fn logcat_toolbar(
    state: State,
    pane: Pane,
    shown: floem::reactive::Memo<Vec<Arc<Entry>>>,
    jump: floem::reactive::RwSignal<Option<usize>>,
    primary_count: usize,
) -> impl View {
    let config = state.window.common.config;
    let mut actions = toolbar_actions(state, pane, shown, jump);
    let secondary = actions.split_off(primary_count);
    let expanded = create_rw_signal(false);
    let chevron = clickable_icon(
        || SparrowIcons::ITEM_CLOSED,
        move || expanded.set(true),
        move || expanded.get(),
        || false,
        || "More Logcat actions",
        config,
    )
    .keyboard_navigable()
    .on_event_cont(EventListener::PointerEnter, move |_| expanded.set(true))
    .on_event(EventListener::KeyDown, move |event| {
        if matches!(event, Event::KeyDown(key) if matches!(key.key.logical_key, Key::Named(NamedKey::Enter | NamedKey::Space))) {
            expanded.update(|open| *open = !*open);
            EventPropagation::Stop
        } else {
            EventPropagation::Continue
        }
    })
    .style(|s| s.size(TOOLBAR_WIDTH, TOOLBAR_BUTTON).flex_shrink(0.0_f32));
    let flyout_width = secondary.len() as f64 * TOOLBAR_BUTTON + 2.0;
    let flyout =
        floem::views::h_stack_from_iter(secondary.into_iter().map(|action| {
            action.style(|s| {
                s.size(TOOLBAR_BUTTON, TOOLBAR_BUTTON).flex_shrink(0.0_f32)
            })
        }))
        .style(move |s| {
            let config = config.get();
            s.width(flyout_width)
                .flex_wrap(floem::taffy::FlexWrap::Wrap)
                .min_width(0.0)
                .background(config.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(4.0)
                .border_color(config.color(SparrowColor::SPARROW_BORDER))
                .apply_if(!expanded.get(), |s| s.hide())
        });
    let trigger = stack((
        Divider::new(config)
            .thickness(TOOLBAR_DIVIDER as f32)
            .indent(6.0)
            .into_view(),
        chevron,
    ))
    .style(|s| s.width(TOOLBAR_WIDTH).flex_col().flex_shrink(0.0_f32));
    let overflow = stack((trigger, flyout))
        .on_event_cont(EventListener::PointerLeave, move |_| expanded.set(false))
        .on_event(EventListener::KeyDown, move |event| {
            if expanded.get() && matches!(event, Event::KeyDown(key) if key.key.logical_key == Key::Named(NamedKey::Escape)) {
                expanded.set(false);
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        })
        .style(|s| s.absolute().inset_left(0.0).inset_bottom(0.0).max_width_full().items_end().z_index(10));
    stack((
        floem::views::v_stack_from_iter(actions.into_iter().map(|action| {
            action.style(|s| {
                s.size(TOOLBAR_BUTTON, TOOLBAR_BUTTON).flex_shrink(0.0_f32)
            })
        }))
        .style(|s| s.width(TOOLBAR_WIDTH).items_center()),
        overflow,
    ))
    .style(|s| s.size_full())
}

#[derive(Clone, PartialEq)]
struct Row {
    entry: Arc<Entry>,
    index: usize,
    message: String,
    format: super::format::Formatting,
    wrap: bool,
}
fn wrap_message(message: &str, columns: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in message.lines() {
        let mut part = String::new();
        let mut width = 0;
        for ch in line.chars() {
            let size = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + size > columns && !part.is_empty() {
                out.push(std::mem::take(&mut part));
                width = 0;
            }
            part.push(ch);
            width += size;
        }
        out.push(part);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
fn log_row(
    row: Row,
    window: std::rc::Rc<crate::window_tab::WindowTabData>,
) -> impl View {
    let config = window.common.config;
    let e = row.entry;
    let system = e.pid == 0 || e.marker;
    let format = row.format;
    let height = format.height() as f32;
    let size = format.font_size as f32;
    let level = e.level;
    let color = match level {
        'V' => Color::from_rgb8(160, 160, 160),
        'D' => Color::from_rgb8(0, 170, 190),
        'I' => Color::from_rgb8(205, 205, 0),
        'W' => Color::from_rgb8(225, 210, 0),
        'E' | 'F' | 'A' => Color::from_rgb8(255, 90, 95),
        _ => Color::from_rgb8(180, 180, 180),
    };
    let text = |value: String, width: f32| {
        label(move || value.clone()).style(move |s| {
            s.width(width)
                .min_width(0.0)
                .height(height)
                .items_center()
                .text_ellipsis()
                .flex_shrink(0.0_f32)
        })
    };
    let first = row.index == 0;
    let timestamp = if first {
        format.time_text(&e.timestamp)
    } else {
        String::new()
    };
    let ids = if first && e.pid > 0 {
        match (format.pid, format.tid) {
            (true, true) => format!("{}-{}", e.pid, e.tid),
            (true, false) => e.pid.to_string(),
            (false, true) => e.tid.to_string(),
            _ => String::new(),
        }
    } else {
        String::new()
    };
    let tag = if first { e.tag.clone() } else { String::new() };
    let package = if first && e.pid > 0 {
        if e.package.is_empty() {
            format!("pid-{}", e.pid)
        } else {
            e.package.clone()
        }
    } else {
        String::new()
    };
    let process = if first && e.pid > 0 {
        e.process.clone()
    } else {
        String::new()
    };
    let tag_color = tag_color(&e.tag);
    let message = row.message;
    let source =
        super::source::java_location(&message, window.workspace.path.as_deref());
    let jump_source = source.clone();
    let command = window.common.internal_command;
    let raw = e.raw.clone();

    stack((
        text(timestamp, if format.date { 200.0 } else { 105.0 })
            .style(move |s| s.apply_if(!format.timestamp || system, |s| s.hide())),
        text(ids, 88.0).style(move |s| {
            s.apply_if((!format.pid && !format.tid) || system, |s| s.hide())
        }),
        text(tag, format.tag_width as f32).style(move |s| {
            s.color(tag_color)
                .apply_if(system || !format.tag, |s| s.hide())
        }),
        text(package, format.package_width as f32)
            .style(move |s| s.apply_if(!format.package || system, |s| s.hide())),
        text(process, format.package_width as f32)
            .style(move |s| s.apply_if(!format.process || system, |s| s.hide())),
        text(
            if first && level != '\0' {
                level.to_string()
            } else {
                String::new()
            },
            24.0,
        )
        .style(move |s| {
            s.apply_if(system || !format.level, |s| s.hide())
                .justify_center()
                .background(if first {
                    color.multiply_alpha(0.55)
                } else {
                    Color::TRANSPARENT
                })
        }),
        super::super::debug_console::row(message, window.clone()).style(move |s| {
            s.height(height)
                .items_center()
                .color(color)
                .selectable(true)
                .apply_if(row.wrap, |s| s.flex_grow(1.0_f32).min_width(0.0))
        }),
    ))
    .on_double_click_stop(move |_| {
        if let Some(location) = jump_source.clone() {
            command
                .send(crate::command::InternalCommand::JumpToLocation { location });
        }
    })
    .ide_context_menu(move || {
        let raw = raw.clone();
        let mut menu = Menu::new("Log entry").entry(
            MenuItem::new("Copy entry")
                .action(move || crate::menus::copy_text(&raw)),
        );
        if let Some(location) = source.clone() {
            menu = menu.entry(MenuItem::new("Go to source").action(move || {
                command.send(crate::command::InternalCommand::JumpToLocation {
                    location: location.clone(),
                })
            }));
        }
        menu
    })
    .style(move |s| {
        s.height(height)
            .min_width_full()
            .items_center()
            .padding_left(10.0)
            .font_family(config.get().terminal.font_family.clone())
            .font_size(size)
    })
}
fn tag_color(tag: &str) -> Color {
    const COLORS: [(u8, u8, u8); 7] = [
        (0, 190, 190),
        (120, 180, 255),
        (210, 170, 230),
        (210, 195, 80),
        (155, 205, 170),
        (200, 170, 150),
        (140, 180, 215),
    ];
    let hash = tag
        .bytes()
        .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let (r, g, b) = COLORS[hash % COLORS.len()];
    Color::from_rgb8(r, g, b)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_wrapping_preserves_text() {
        assert_eq!(wrap_message("abc世界", 4), vec!["abc", "世界"]);
        assert_eq!(wrap_message("", 10), vec![""]);
    }
}
