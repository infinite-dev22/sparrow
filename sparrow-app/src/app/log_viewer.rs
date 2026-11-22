//! Read-only viewer for Sparrow's rolling logs and crash reports.

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use floem::{
    View,
    event::EventListener,
    ext_event::{create_ext_action, create_trigger, register_ext_trigger},
    reactive::{
        SignalGet, SignalUpdate, SignalWith, batch, create_effect, create_memo,
        create_rw_signal,
    },
    views::{Decorators, container, label, stack, virtual_stack},
};
use notify::{PollWatcher, RecursiveMode, Watcher};
use sparrow_core::directory::Directory;

use super::logging::{CrashEntry, CrashReport};

use crate::{
    config::color::SparrowColor,
    popup_menu::{Menu, MenuItem, PopupMenuExt},
    widgets::{
        ButtonKind, Divider, button, dialog_surface, sparrow_scroll,
        text_input::text_input,
    },
    window_tab::WindowTabData,
};

const MAX_BYTES: u64 = 512 * 1024;
const MAX_ENTRIES: usize = 2500;

mod details;
mod entry;

use details::DetailTab;
use entry::{Entry, Level, parse_entries};

#[derive(Default)]
struct Snapshot {
    files: Vec<String>,
    file: Option<String>,
    entries: Vec<Arc<Entry>>,
    limited: bool,
}

fn file_stamp(name: &str) -> Option<(chrono::NaiveDate, bool)> {
    let stem = name.strip_suffix(".log")?;
    let (date, crash) = if let Some(date) = stem.strip_prefix("crash-sparrow.") {
        (date, true)
    } else {
        (stem.strip_prefix("sparrow.")?, false)
    };
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .map(|date| (date, crash))
}

fn read_logs(directory: &Path, preferred: Option<&str>) -> Result<Snapshot, String> {
    let read_dir = fs::read_dir(directory)
        .map_err(|error| format!("Could not list logs: {error}"))?;
    let mut dated_files = Vec::new();
    for entry in read_dir.flatten() {
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        if let Some(stamp) = file_stamp(&name)
            && entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            dated_files.push((stamp, name));
        }
    }
    dated_files.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let file = preferred
        .and_then(|name| dated_files.iter().find(|(_, file)| file == name))
        .or_else(|| dated_files.iter().find(|((_, crash), _)| !crash))
        .or_else(|| dated_files.first())
        .map(|(_, file)| file.clone());
    let files = dated_files.into_iter().map(|(_, name)| name).collect();
    let Some(name) = &file else {
        return Ok(Snapshot {
            files,
            ..Snapshot::default()
        });
    };
    let (mut entries, mut limited) = read_tail(directory.join(name))
        .map_err(|error| format!("Could not read {name}: {error}"))?;
    if entries.len() > MAX_ENTRIES {
        limited = true;
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    Ok(Snapshot {
        files,
        file,
        entries: entries.into_iter().rev().map(Arc::new).collect(),
        limited,
    })
}

fn read_tail(path: PathBuf) -> std::io::Result<(Vec<Entry>, bool)> {
    let mut file = File::open(path)?;
    let start = file.metadata()?.len().saturating_sub(MAX_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES).read_to_end(&mut bytes)?;
    let bytes = if start == 0 {
        bytes.as_slice()
    } else {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.as_slice(), |index| &bytes[index + 1..])
    };
    Ok((parse_entries(&String::from_utf8_lossy(bytes)), start > 0))
}

fn read_crash_entry(path: &Path, target: &CrashEntry) -> std::io::Result<Entry> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(target.offset))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    if text.lines().next() == Some(target.header.as_str()) {
        if let Some(entry) = parse_entries(&text).into_iter().next() {
            return Ok(entry);
        }
    }
    Err(std::io::Error::other(
        "The selected crash entry is no longer in the log.",
    ))
}

fn open_snapshot(
    directory: &Path,
    file: Option<&str>,
    report: Option<&CrashReport>,
) -> Result<(Snapshot, Option<usize>), String> {
    let requested = report.map(|report| report.file.as_str()).or(file);
    let mut snapshot = read_logs(directory, requested)?;
    let Some(report) = report else {
        return Ok((snapshot, None));
    };
    if snapshot.file.as_deref() != Some(report.file.as_str()) {
        return Err(format!(
            "{} is no longer in the log directory.",
            report.file
        ));
    }
    let Some(target) = &report.entry else {
        return Ok((snapshot, None));
    };
    let entry = read_crash_entry(&directory.join(&report.file), target)
        .map_err(|error| format!("Could not open the crash report. {error}"))?;
    let index = snapshot.entries.iter().position(|row| **row == entry);
    if let Some(index) = index {
        Ok((snapshot, Some(index)))
    } else {
        // The notice can outlive the viewer's bounded tail of a growing log.
        snapshot.entries.insert(0, Arc::new(entry));
        Ok((snapshot, Some(0)))
    }
}

pub(super) fn view(window: Rc<WindowTabData>) -> impl View {
    let visible = window.log_viewer_visible;
    let crash_report = window.log_viewer_report;
    let config = window.common.config;
    let workbench = window.common.workbench_command;
    let scope = window.scope;
    let snapshot = create_rw_signal(Snapshot::default());
    let chosen_file = create_rw_signal(None::<String>);
    let minimum_level = create_rw_signal(Level::Other);
    let query = create_rw_signal(String::new());
    let refresh = create_rw_signal(0_u64);
    let live = create_rw_signal(false);
    let generation = create_rw_signal(0_u64);
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let selection = create_rw_signal(None::<Arc<Entry>>);
    let detail_tab = create_rw_signal(DetailTab::Overview);
    let body_width = create_rw_signal(0.0);
    let target_row = create_rw_signal(None::<usize>);
    let search = text_input(query, config)
        .placeholder("Search log messages")
        .style(|s| s.flex_grow(1.0_f32).flex_basis(0.0));
    let search_id = search.id();

    let watcher = Rc::new(RefCell::new(None::<PollWatcher>));
    let change = create_trigger();
    let dirty = Arc::new(AtomicBool::new(false));
    let dirty_on_ui = dirty.clone();
    create_effect(move |_| {
        change.track();
        if dirty_on_ui.swap(false, Ordering::Relaxed)
            && visible.get_untracked()
            && live.get_untracked()
        {
            refresh.update(|count| *count += 1);
        }
    });
    let active_watcher = watcher.clone();
    create_effect(move |_| {
        active_watcher.borrow_mut().take();
        if !visible.get() || !live.get() {
            return;
        }
        let result = (|| {
            let dir = Directory::logs_directory().ok_or_else(|| {
                "Sparrow's log directory is unavailable.".to_owned()
            })?;
            let dirty = dirty.clone();
            let mut watcher = PollWatcher::new(
                move |event: notify::Result<notify::Event>| {
                    if event.is_ok_and(|event| {
                        event.paths.iter().any(|path| {
                            path.file_name()
                                .and_then(|name| name.to_str())
                                .and_then(file_stamp)
                                .is_some()
                        })
                    }) && !dirty.swap(true, Ordering::Relaxed)
                    {
                        // Floem delivers this trigger on the UI thread.
                        register_ext_trigger(change);
                    }
                },
                notify::Config::default().with_poll_interval(Duration::from_secs(2)),
            )
            .map_err(|error| error.to_string())?;
            watcher
                .watch(&dir, RecursiveMode::NonRecursive)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(watcher)
        })();
        match result {
            Ok(watcher) => {
                active_watcher.replace(Some(watcher));
                refresh.update(|count| *count += 1);
            }
            Err(problem) => {
                live.set(false);
                error.set(Some(format!("Could not start live updates. {problem}")));
            }
        }
    });

    create_effect(move |_| {
        if !visible.get() {
            return;
        }
        let report = crash_report.get();
        let requested = chosen_file.get();
        refresh.get();
        let ticket = generation.get_untracked() + 1;
        generation.set(ticket);
        loading.set(true);
        let send = create_ext_action(
            scope,
            move |result: Result<(Snapshot, Option<usize>), String>| {
                if visible.try_get_untracked() != Some(true)
                    || generation.try_get_untracked() != Some(ticket)
                {
                    return;
                }
                loading.set(false);
                match result {
                    Ok((mut data, index)) => batch(|| {
                        // Keep the active row and its view when a live refresh
                        // returns records already in the current snapshot.
                        snapshot.with_untracked(|previous| {
                            if previous.file == data.file {
                                let mut cached = HashMap::<_, VecDeque<_>>::new();
                                for row in &previous.entries {
                                    cached
                                        .entry(row.raw.as_str())
                                        .or_default()
                                        .push_back(row);
                                }
                                for row in &mut data.entries {
                                    if let Some(old) = cached
                                        .get_mut(row.raw.as_str())
                                        .and_then(VecDeque::pop_front)
                                    {
                                        *row = old.clone();
                                    }
                                }
                            }
                        });
                        let selected = index
                            .map(|index| data.entries[index].clone())
                            .or_else(|| {
                                selection.get_untracked().filter(|selected| {
                                    data.entries
                                        .iter()
                                        .any(|entry| Arc::ptr_eq(entry, selected))
                                })
                            });
                        if selection.with_untracked(|old| match (old, &selected) {
                            (Some(old), Some(new)) => !Arc::ptr_eq(old, new),
                            (None, None) => false,
                            _ => true,
                        }) {
                            selection.set(selected);
                        }
                        error.set(None);
                        snapshot.set(data);
                        target_row.set(index);
                    }),
                    Err(message) => batch(|| {
                        selection.set(None);
                        snapshot.set(Snapshot::default());
                        target_row.set(None);
                        error.set(Some(message));
                    }),
                }
            },
        );
        if let Err(problem) = std::thread::Builder::new()
            .name("SparrowLogViewer".into())
            .spawn(move || {
                let result = Directory::logs_directory()
                    .ok_or_else(|| "Sparrow's log directory is unavailable.".into())
                    .and_then(|dir| {
                        open_snapshot(&dir, requested.as_deref(), report.as_ref())
                    });
                send(result);
            })
        {
            loading.set(false);
            error.set(Some(format!("Could not start the log reader: {problem}")));
        }
    });
    create_effect(move |_| {
        if visible.get() {
            if crash_report.with(Option::is_some) {
                query.set(String::new());
                minimum_level.set(Level::Other);
                detail_tab.set(DetailTab::Technical);
                live.set(false);
            } else {
                detail_tab.set(DetailTab::Overview);
            }
            search_id.request_focus();
        }
    });
    let filtered = create_memo(move |_| {
        let query = query.get().to_lowercase();
        let level = minimum_level.get();
        snapshot.with(|snapshot| {
            snapshot
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.level >= level && entry.searchable.contains(query.trim())
                })
                .map(|(index, entry)| (index, Arc::clone(entry)))
                .collect::<im::Vector<_>>()
        })
    });
    let reveal = move |entry: Arc<Entry>| {
        batch(|| {
            if !filtered.with_untracked(|rows| {
                rows.iter().any(|(_, row)| Arc::ptr_eq(row, &entry))
            }) {
                query.set(String::new());
                minimum_level.set(Level::Other);
            }
            selection.set(Some(entry.clone()));
        });
        target_row.set(filtered.with_untracked(|rows| {
            rows.iter().position(|(_, row)| Arc::ptr_eq(row, &entry))
        }));
    };
    let rows = virtual_stack(
        move || filtered.get(),
        |(index, entry)| (*index, Arc::as_ptr(entry) as usize),
        move |(_, entry)| {
            let selected = entry.clone();
            let highlight = entry.clone();
            let time = entry.time.clone();
            let summary = entry.summary.clone();
            let level = entry.level;
            stack((
                label(move || time.clone()).style(move |s| {
                    s.width(94.0).flex_shrink(0.0_f32).color(
                        config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                    )
                }),
                label(move || {
                    if level == Level::Other {
                        ""
                    } else {
                        level.label()
                    }
                })
                .style(move |s| {
                    s.width(58.0)
                        .flex_shrink(0.0_f32)
                        .font_bold()
                        .color(config.get().color(level.color()))
                }),
                label(move || summary.clone())
                    .style(|s| s.min_width(0.0).text_ellipsis()),
            ))
            .on_click_stop(move |_| {
                batch(|| {
                    if selection.with_untracked(Option::is_none) {
                        detail_tab.set(DetailTab::Overview);
                    }
                    selection.set(Some(selected.clone()));
                });
            })
            .style(move |s| {
                let cfg = config.get();
                s.height(24.0)
                    .width_full()
                    .min_width(0.0)
                    .items_center()
                    .padding_horiz(8.0)
                    .apply_if(
                        selection.with(|row| {
                            row.as_ref()
                                .is_some_and(|row| Arc::ptr_eq(row, &highlight))
                        }),
                        |s| {
                            s.background(
                                cfg.color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                            )
                        },
                    )
                    .hover(|s| {
                        s.background(
                            cfg.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
            })
        },
    )
    .item_size_fixed(|| 24.0)
    .style(|s| s.absolute().width_full().flex_col());
    let row_list = sparrow_scroll(rows)
        .scroll_to(move || {
            target_row
                .get()
                .map(|index| (0.0, index as f64 * 24.0).into())
        })
        .style(move |s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0.0)
                .min_height(0.0)
                .width_full()
                .background(config.get().color(SparrowColor::EDITOR_BACKGROUND))
        });

    let list = stack((
        stack((
            label(|| "Time UTC").style(|s| s.width(94.0).flex_shrink(0.0_f32)),
            label(|| "Level").style(|s| s.width(58.0).flex_shrink(0.0_f32)),
            label(|| "Message").style(|s| s.min_width(0.0)),
        ))
        .style(move |s| {
            s.width_full()
                .height(26.0)
                .items_center()
                .padding_horiz(8.0)
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        Divider::new(config).into_view(),
        row_list,
    ))
    .style(|s| {
        s.flex_col()
            .flex_grow(1.0_f32)
            .flex_basis(0.0)
            .min_height(0.0)
            .min_width(0.0)
    });
    let inspection = stack((
        list,
        details::pane(
            selection,
            create_memo(move |_| snapshot.with(|data| data.entries.clone())),
            create_memo(move |_| snapshot.with(|data| data.file.clone())),
            detail_tab,
            query,
            reveal,
            config,
        )
        .style(move |s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0.0)
                .apply_if(selection.with(Option::is_none), |s| s.hide())
        }),
    ))
    .on_resize(move |rect| body_width.set(rect.width()))
    .style(move |s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_basis(0.0)
            .min_height(0.0)
            .gap(10.0)
            .apply_if(body_width.get() < 680.0, |s| s.flex_col())
    });
    let body = stack((
        stack((
            label(|| "IDE Logs")
                .style(|s| s.font_bold().font_size(17.0).flex_grow(1.0_f32)),
            button(|| "Open Log Folder", ButtonKind::Plain, config).on_click_stop(
                move |_| {
                    workbench.send(
                        crate::command::SparrowWorkbenchCommand::OpenLogsDirectory,
                    )
                },
            ),
            button(|| "Close", ButtonKind::Plain, config)
                .on_click_stop(move |_| visible.set(false)),
        ))
        .style(|s| s.items_center().width_full().gap(8.0)),
        stack((
            button(
                move || {
                    snapshot
                        .with(|data| data.file.clone())
                        .unwrap_or_else(|| "Log files".into())
                },
                ButtonKind::Outlined,
                config,
            )
            .ide_popout_menu(move || {
                snapshot.with(|data| {
                    data.files.iter().fold(
                        Menu::new("Log files").entry(
                            MenuItem::new("Latest IDE log").action(move || {
                                batch(|| {
                                    crash_report.set(None);
                                    chosen_file.set(None);
                                });
                            }),
                        ),
                        |menu, file| {
                            let file = file.clone();
                            menu.entry(MenuItem::new(file.clone()).action(
                                move || {
                                    batch(|| {
                                        crash_report.set(None);
                                        chosen_file.set(Some(file.clone()));
                                    })
                                },
                            ))
                        },
                    )
                })
            }),
            button(
                move || match minimum_level.get() {
                    Level::Other => "All levels".to_owned(),
                    level => format!("{} and above", level.label()),
                },
                ButtonKind::Outlined,
                config,
            )
            .ide_popout_menu(move || {
                Level::ALL.into_iter().fold(
                    Menu::new("Minimum level"),
                    |menu, level| {
                        menu.entry(
                            MenuItem::new(if level == Level::Other {
                                "All levels"
                            } else {
                                level.label()
                            })
                            .action(move || minimum_level.set(level)),
                        )
                    },
                )
            }),
            search,
            button(
                move || {
                    if live.get() {
                        "Pause live"
                    } else {
                        "Start live"
                    }
                },
                ButtonKind::Outlined,
                config,
            )
            .on_click_stop(move |_| live.update(|active| *active = !*active)),
            button(|| "Refresh", ButtonKind::Outlined, config)
                .on_click_stop(move |_| refresh.update(|count| *count += 1)),
        ))
        .style(|s| {
            s.items_center()
                .width_full()
                .gap(8.0)
                .flex_wrap(floem::style::FlexWrap::Wrap)
        }),
        label(move || {
            if let Some(error) = error.get() {
                return error;
            }
            if loading.get() && snapshot.with(|data| data.file.is_none()) {
                return "Reading logs…".into();
            }
            if let Some(problem) = super::logging::status() {
                return problem;
            }
            let count = filtered.with(|entries| entries.len());
            if count == 0 {
                return if snapshot.with(|data| data.entries.is_empty()) {
                    "No log entries yet".into()
                } else {
                    "No matching log entries".into()
                };
            }
            if snapshot.with(|data| data.limited) {
                format!("{count} entries in the recent log excerpt")
            } else {
                let noun = if count == 1 { "entry" } else { "entries" };
                format!("{count} {noun}")
            }
        })
        .style(move |s| {
            s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        inspection,
    ))
    .on_event_stop(EventListener::PointerDown, |_| {})
    .style(move |s| {
        dialog_surface(s, &config.get())
            .width(1120.0)
            .max_width_pct(94.0)
            .height_pct(88.0)
            .min_height(0.0)
            .flex_col()
            .padding(14.0)
            .gap(10.0)
    });

    container(body)
        .on_cleanup(move || drop(watcher.borrow_mut().take()))
        .on_event_stop(EventListener::PointerDown, move |_| visible.set(false))
        .style(move |s| {
            s.absolute()
                .size_full()
                .items_center()
                .justify_center()
                .z_index(50)
                .background(
                    config
                        .get()
                        .color(SparrowColor::SPARROW_DROPDOWN_SHADOW)
                        .multiply_alpha(0.45),
                )
                .apply_if(!visible.get(), |s| s.hide())
        })
        .debug_name("IDE Logs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_logs_and_preserves_multiline_details() {
        let log = "2026-11-22T03:04:05.123456Z  WARN sparrow_app: fixture\n  detail\n2026-11-22T03:04:06Z ERROR main ThreadId(1) sparrow_app: app.rs:20: broken";
        let rows = parse_entries(log);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].level.label(), "WARN");
        assert_eq!(rows[0].time, "03:04:05.123");
        assert!(rows[0].searchable.contains("fixture\n  detail"));
        assert_eq!(rows[1].level.label(), "ERROR");
        assert_eq!(rows[1].time, "03:04:06.000");
        assert_eq!(rows[1].summary, "broken");
        assert!(rows[1].raw.contains("app.rs:20: broken"));
    }

    #[test]
    fn tails_latest_file_and_reads_selected_crash_report() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("unrelated.log"), "ignore").unwrap();
        let crash = "crash-sparrow.2026-11-22.log";
        fs::write(
            dir.path().join(crash),
            "2026-11-22T03:04:06Z ERROR panic\nbacktrace",
        )
        .unwrap();
        let large = "2026-11-21T03:04:06Z INFO older\n".repeat(20_000);
        fs::write(dir.path().join("sparrow.2026-11-21.log"), large).unwrap();
        let report = read_logs(dir.path(), None).unwrap();
        assert_eq!(report.file.as_deref(), Some("sparrow.2026-11-21.log"));
        assert!(report.limited);
        assert_eq!(report.entries.len(), MAX_ENTRIES);
        assert_eq!(report.files.len(), 2);
        assert!(
            read_logs(dir.path(), Some(crash)).unwrap().entries[0]
                .raw
                .ends_with("backtrace")
        );
    }

    #[test]
    fn opens_the_report_linked_by_the_notice_even_after_another_append() {
        let dir = tempfile::tempdir().unwrap();
        let file = "crash-sparrow.2026-11-27.log";
        let previous = "2026-11-27T12:00:01Z ERROR panic one\nfirst backtrace\n";
        let target = "2026-11-27T18:00:05Z ERROR panic two";
        let later = "2026-11-27T21:00:01Z ERROR panic three";
        let link = CrashReport {
            file: file.into(),
            entry: Some(CrashEntry {
                offset: previous.len() as u64,
                header: target.into(),
                timestamp: "2026-11-27T18:00:05Z".parse().unwrap(),
            }),
        };
        let contents = format!("{previous}{target}\nsecond backtrace\n{later}\n");
        fs::write(dir.path().join(file), &contents).unwrap();
        let (snapshot, selected) =
            open_snapshot(dir.path(), None, Some(&link)).unwrap();
        assert_eq!(selected, Some(1));
        assert_eq!(
            snapshot.entries[selected.unwrap()].raw,
            format!("{target}\nsecond backtrace"),
        );

        let more = format!("{later}\n").repeat(20_000);
        fs::write(dir.path().join(file), format!("{contents}{more}")).unwrap();
        let (snapshot, selected) =
            open_snapshot(dir.path(), None, Some(&link)).unwrap();
        assert!(snapshot.limited);
        assert_eq!(snapshot.entries[selected.unwrap()].summary, "panic two");
    }
}
