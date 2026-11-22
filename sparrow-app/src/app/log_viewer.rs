//! Read-only viewer for Sparrow's rolling logs and crash reports.

use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use floem::{
    View,
    event::EventListener,
    ext_event::create_ext_action,
    reactive::{
        SignalGet, SignalUpdate, SignalWith, batch, create_effect, create_memo,
        create_rw_signal,
    },
    views::{
        Decorators, container,
        editor::{core::register::Clipboard, text::SystemClipboard},
        label, stack, virtual_stack,
    },
};
use sparrow_core::directory::Directory;

use super::logging::{CrashEntry, CrashReport};

use crate::{
    config::color::SparrowColor,
    popup_menu::{Menu, MenuItem, PopupMenuExt},
    widgets::{
        ButtonKind, button, dialog_surface, sparrow_scroll, text_input::text_input,
    },
    window_tab::WindowTabData,
};

const MAX_BYTES: u64 = 512 * 1024;
const MAX_ENTRIES: usize = 2500;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Other,
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Self::Other => "All levels",
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(PartialEq)]
struct Entry {
    time: String,
    level: Level,
    summary: String,
    raw: String,
    searchable: String,
}

#[derive(Default)]
struct Snapshot {
    files: Vec<String>,
    file: Option<String>,
    entries: Vec<Arc<Entry>>,
    limited: bool,
}

fn parse_header(
    line: &str,
) -> Option<(chrono::DateTime<chrono::FixedOffset>, Level, &str)> {
    let (timestamp, rest) = line.split_once(' ')?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
    let (level, message) = rest.trim_start().split_once(' ')?;
    let level = match level {
        "TRACE" => Level::Trace,
        "DEBUG" => Level::Debug,
        "INFO" => Level::Info,
        "WARN" => Level::Warn,
        "ERROR" => Level::Error,
        _ => return None,
    };
    let message = message.trim_start();
    let summary = message.split_once(": ").map_or(message, |(_, body)| {
        match body.split_once(": ") {
            Some((location, text))
                if location.rsplit_once(':').is_some_and(|(file, line)| {
                    (file.contains('/')
                        || file.contains('\\')
                        || file.ends_with(".rs"))
                        && line.parse::<u32>().is_ok()
                }) =>
            {
                text
            }
            _ => body,
        }
    });
    Some((timestamp, level, summary))
}

fn parse_entries(input: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in input.lines() {
        // Keep the viewer and clipboard usable when a tool emitted control codes.
        let line: String = line
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        if let Some((time, level, summary)) = parse_header(&line) {
            entries.push(Entry {
                time: time.format("%H:%M:%S%.3f").to_string(),
                level,
                summary: summary.into(),
                raw: line,
                searchable: String::new(),
            });
        } else if let Some(entry) = entries.last_mut() {
            entry.raw.push('\n');
            entry.raw.push_str(&line);
        } else {
            entries.push(Entry {
                time: String::new(),
                level: Level::Other,
                summary: line.clone(),
                raw: line,
                searchable: String::new(),
            });
        }
    }
    for entry in &mut entries {
        entry.searchable = entry.raw.to_lowercase();
    }
    entries
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
    let generation = create_rw_signal(0_u64);
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let selection = create_rw_signal(None::<Arc<Entry>>);
    let target_row = create_rw_signal(None::<usize>);
    let search = text_input(query, config)
        .placeholder("Search log messages")
        .style(|s| s.flex_grow(1.0_f32).flex_basis(0.0));
    let search_id = search.id();

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
                    Ok((data, index)) => batch(|| {
                        selection
                            .set(index.map(|index| data.entries[index].clone()));
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
                    let accent = match level {
                        Level::Error => SparrowColor::SPARROW_ERROR,
                        Level::Warn => SparrowColor::SPARROW_WARN,
                        Level::Info => SparrowColor::EDITOR_LINK,
                        _ => SparrowColor::PANEL_FOREGROUND_DIM,
                    };
                    s.width(58.0)
                        .flex_shrink(0.0_f32)
                        .font_bold()
                        .color(config.get().color(accent))
                }),
                label(move || summary.clone())
                    .style(|s| s.min_width(0.0).text_ellipsis()),
            ))
            .on_click_stop(move |_| selection.set(Some(selected.clone())))
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
                    data.files
                        .iter()
                        .fold(Menu::new("Log files"), |menu, file| {
                            let file = file.clone();
                            menu.entry(MenuItem::new(file.clone()).action(
                                move || {
                                    batch(|| {
                                        crash_report.set(None);
                                        chosen_file.set(Some(file.clone()));
                                    })
                                },
                            ))
                        })
                })
            }),
            button(
                move || minimum_level.get().label(),
                ButtonKind::Outlined,
                config,
            )
            .ide_popout_menu(move || {
                [Level::Other, Level::Info, Level::Warn, Level::Error]
                    .into_iter()
                    .fold(Menu::new("Minimum level"), |menu, level| {
                        menu.entry(
                            MenuItem::new(level.label())
                                .action(move || minimum_level.set(level)),
                        )
                    })
            }),
            search,
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
            if loading.get() {
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
        row_list,
        stack((
            button(|| "Copy entry", ButtonKind::Plain, config)
                .on_click_stop(move |_| {
                    if let Some(entry) = selection.get_untracked() {
                        SystemClipboard::new().put_string(&entry.raw);
                    }
                })
                .style(|s| s.align_self(Some(floem::style::AlignItems::FlexEnd))),
            sparrow_scroll(
                label(move || {
                    selection.with(|entry| {
                        entry
                            .as_ref()
                            .map_or_else(String::new, |entry| entry.raw.clone())
                    })
                })
                .style(|s| s.width_full().selectable(true)),
            )
            .style(|s| s.width_full().min_height(0.0).max_height(126.0)),
        ))
        .style(move |s| {
            s.flex_col()
                .width_full()
                .apply_if(selection.with(Option::is_none), |s| s.hide())
        }),
    ))
    .on_event_stop(EventListener::PointerDown, |_| {})
    .style(move |s| {
        dialog_surface(s, &config.get())
            .width(920.0)
            .max_width_pct(94.0)
            .height_pct(82.0)
            .min_height(0.0)
            .flex_col()
            .padding(14.0)
            .gap(10.0)
    });

    container(body)
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
