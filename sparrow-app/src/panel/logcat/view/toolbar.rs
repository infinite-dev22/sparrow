//! Capture commands and the overflow flyout.

use crate::panel::logcat::{
    Buffer, Pane, State,
    model::{self, Entry},
};
use crate::widgets::divider::Divider;
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    widgets::popup_menu::{Menu, MenuItem, PopupMenuExt},
};
use floem::{
    IntoView, View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    style::CursorStyle,
    views::{Decorators, label, stack},
};
use std::sync::Arc;

fn formatting_menu(pane: Pane, state: State) -> Menu {
    let mut menu = Menu::new("Logcat formatting");
    for (title, compact) in [("Standard", false), ("Compact", true)] {
        let pane = pane.clone();
        let state = state.clone();
        menu = menu.entry(MenuItem::new(title).action(move || {
            pane.compact.set(compact);
            pane.format.set(if compact {
                crate::panel::logcat::format::Formatting::compact()
            } else {
                crate::panel::logcat::format::Formatting::default()
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
                                    crate::panel::logcat::archive::encode(&entries)
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

pub(super) const TOOLBAR_BUTTON: f64 = 26.0;
pub(super) const TOOLBAR_WIDTH: f64 = 34.0;
pub(super) const TOOLBAR_DIVIDER: f64 = 1.0;
pub(super) const PRIMARY_ACTIONS: usize = 7;

pub(super) fn logcat_toolbar(
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
