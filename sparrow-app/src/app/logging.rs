use std::{
    io::{self, BufRead, BufReader, Write},
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use sparrow_core::directory::Directory;
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::{
    ErrorCounter, NonBlockingBuilder, WorkerGuard,
};
use tracing_subscriber::{filter::Targets, reload::Handle};

use crate::tracing::*;

struct LogHealth {
    startup_error: Option<String>,
    dropped: Option<ErrorCounter>,
    write_errors: Arc<AtomicUsize>,
}

static HEALTH: OnceLock<LogHealth> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashReport {
    pub(crate) file: String,
    pub(crate) entry: Option<CrashEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CrashEntry {
    pub(crate) offset: u64,
    pub(crate) header: String,
    pub(crate) timestamp: chrono::DateTime<chrono::Utc>,
}

impl CrashReport {
    pub(crate) fn notice(&self) -> String {
        self.entry.as_ref().map_or_else(
            || format!("A Sparrow panic was recorded in {}.", self.file),
            |entry| {
                format!(
                    "Sparrow crashed at {}.",
                    entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f UTC")
                )
            },
        )
    }
}

pub(crate) fn take_previous_crash_notice() -> Option<CrashReport> {
    static CHECKED: AtomicBool = AtomicBool::new(false);
    if CHECKED.swap(true, Ordering::Relaxed) {
        return None;
    }
    let directory = Directory::logs_directory()?;
    unreported_crash(&directory).ok().flatten()
}

fn unreported_crash(directory: &Path) -> io::Result<Option<CrashReport>> {
    let Some(report) = crash_reports(directory)?.pop() else {
        return Ok(None);
    };
    let name = report.file_name().to_string_lossy().into_owned();
    let metadata = report.metadata()?;
    let stamp = format!("{name}\n{}\n{:?}", metadata.len(), metadata.modified()?);
    let marker = directory.join(".last-crash-notice");
    if std::fs::read_to_string(&marker).is_ok_and(|seen| seen == stamp) {
        return Ok(None);
    }
    let entry = latest_crash_entry(&report.path())?;
    if let Err(error) = std::fs::write(marker, stamp) {
        tracing::debug!(%error, "Could not save the last crash notification marker");
    }
    Ok(Some(CrashReport { file: name, entry }))
}

fn latest_crash_entry(path: &Path) -> io::Result<Option<CrashEntry>> {
    let mut reader = BufReader::new(std::fs::File::open(path)?);
    let mut line = Vec::new();
    let mut offset = 0;
    let mut latest = None;
    loop {
        line.clear();
        let bytes = reader.read_until(b'\n', &mut line)?;
        if bytes == 0 {
            return Ok(latest);
        }
        if let Ok(header) = std::str::from_utf8(&line) {
            let header = header.trim_end_matches(['\r', '\n']);
            if let Some((timestamp, _)) =
                header.split_once(" ERROR sparrow_app::panic_hook: process ")
            {
                if let Ok(timestamp) =
                    chrono::DateTime::parse_from_rfc3339(timestamp)
                {
                    latest = Some(CrashEntry {
                        offset,
                        header: header.into(),
                        timestamp: timestamp.to_utc(),
                    });
                }
            }
        }
        offset += bytes as u64;
    }
}

pub(crate) fn status() -> Option<String> {
    let health = HEALTH.get()?;
    if let Some(error) = &health.startup_error {
        return Some(error.clone());
    }
    let dropped = health
        .dropped
        .as_ref()
        .map_or(0, ErrorCounter::dropped_lines);
    let errors = health.write_errors.load(Ordering::Relaxed);
    (errors > 0 || dropped > 0).then(|| {
        format!(
            "Log writer recorded {errors} I/O errors and dropped {dropped} lines."
        )
    })
}

struct MonitoredWriter<W>(W, Arc<AtomicUsize>);

impl<W: Write> Write for MonitoredWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes).inspect_err(|_| {
            self.1.fetch_add(1, Ordering::Relaxed);
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush().inspect_err(|_| {
            self.1.fetch_add(1, Ordering::Relaxed);
        })
    }
}

#[inline(always)]
pub(super) fn logging() -> (Handle<Targets>, Option<WorkerGuard>) {
    use tracing_subscriber::{filter, fmt, prelude::*, reload};

    let write_errors = Arc::new(AtomicUsize::new(0));
    let appender = Directory::logs_directory()
        .ok_or_else(|| "Sparrow's log directory is unavailable.".to_string())
        .and_then(|dir| {
            tracing_appender::rolling::Builder::new()
                .max_log_files(10)
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("sparrow")
                .filename_suffix("log")
                .build(dir)
                .map_err(|error| {
                    format!("Could not open the Sparrow log directory: {error}")
                })
        });
    let (log_file, guard, startup_error) = match appender {
        Ok(appender) => {
            let (writer, guard) = NonBlockingBuilder::default()
                .buffered_lines_limit(8192)
                .finish(MonitoredWriter(appender, write_errors.clone()));
            (Some(writer), Some(guard), None)
        }
        Err(error) => {
            eprintln!("{error}");
            (None, None, Some(error))
        }
    };
    let _ = HEALTH.set(LogHealth {
        startup_error,
        dropped: log_file.as_ref().map(|writer| writer.error_counter()),
        write_errors,
    });

    let log_file_filter_targets = filter::Targets::new()
        .with_target("sparrow_app", LevelFilter::DEBUG)
        .with_target("sparrow_proxy", LevelFilter::DEBUG)
        .with_target("sparrow_core", LevelFilter::DEBUG)
        .with_default(LevelFilter::from_level(TraceLevel::INFO));
    let (log_file_filter, reload_handle) =
        reload::Subscriber::new(log_file_filter_targets);

    let console_filter_targets = std::env::var("SPARROW_LOG")
        .unwrap_or_default()
        .parse::<filter::Targets>()
        .unwrap_or_default();

    let registry = tracing_subscriber::registry();
    if let Some(log_file) = log_file {
        let file_layer = tracing_subscriber::fmt::subscriber()
            .with_ansi(false)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_file(true)
            .with_line_number(true)
            .with_writer(log_file)
            .with_filter(log_file_filter);
        registry
            .with(file_layer)
            .with(
                fmt::Subscriber::default()
                    .with_line_number(true)
                    .with_target(true)
                    .with_thread_names(true)
                    .with_filter(console_filter_targets),
            )
            .init();
    } else {
        registry
            .with(fmt::Subscriber::default().with_filter(console_filter_targets))
            .init();
    };

    (reload_handle, guard)
}

pub(super) fn panic_hook() {
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("main");
        let backtrace = backtrace::Backtrace::new();

        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");

        let crash = format!(
            "{} ERROR sparrow_app::panic_hook: process {} ({}) thread {thread} panicked at {}: {payload}\n{backtrace:?}\n",
            chrono::Utc::now().to_rfc3339(),
            std::process::id(),
            sparrow_core::meta::VERSION,
            info.location()
                .map_or_else(|| "unknown location".into(), ToString::to_string),
        );
        if let Err(error) = Directory::logs_directory()
            .ok_or_else(|| io::Error::other("log directory unavailable"))
            .and_then(|dir| write_crash_log(&dir, &crash))
        {
            eprintln!(
                "Could not persist the Sparrow crash report: {error}\n{crash}"
            );
        }

        match info.location() {
            Some(loc) => {
                trace!(
                    target: "sparrow_app::panic_hook",
                    TraceLevel::ERROR,
                    "thread {thread} panicked at {} | file://./{}:{}:{}\n{:?}",
                    payload,
                    loc.file(), loc.line(), loc.column(),
                    backtrace,
                );
            }
            None => {
                trace!(
                    target: "sparrow_app::panic_hook",
                    TraceLevel::ERROR,
                    "thread {thread} panicked at {}\n{:?}",
                    payload,
                    backtrace,
                );
            }
        }

        #[cfg(windows)]
        error_modal("Error", &info.to_string());

        #[cfg(unix)]
        error_notification("Error", &info.to_string());
    }))
}

fn write_crash_log(directory: &Path, report: &str) -> io::Result<()> {
    // The tracing worker may be unavailable during a panic.
    let path = directory.join(format!(
        "crash-sparrow.{}.log",
        chrono::Utc::now().date_naive()
    ));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(report.as_bytes())?;
    file.sync_data()?;

    let old_reports = crash_reports(directory)?;
    let prune = old_reports.len().saturating_sub(10);
    for entry in old_reports.into_iter().take(prune) {
        std::fs::remove_file(entry.path())?;
    }
    Ok(())
}

fn crash_reports(directory: &Path) -> io::Result<Vec<std::fs::DirEntry>> {
    let mut reports = std::fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry.file_name().to_str().is_some_and(|name| {
                    name.strip_prefix("crash-sparrow.")
                        .and_then(|name| name.strip_suffix(".log"))
                        .is_some_and(|date| {
                            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                                .is_ok()
                        })
                })
        })
        .collect::<Vec<_>>();
    reports.sort_unstable_by_key(|entry| entry.file_name());
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_report_is_saved_and_announced_once_until_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unreported_crash(dir.path()).unwrap(), None);
        write_crash_log(dir.path(), "test panic\n").unwrap();
        let report = unreported_crash(dir.path()).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(&report.file)).unwrap(),
            "test panic\n"
        );
        assert_eq!(unreported_crash(dir.path()).unwrap(), None);
        write_crash_log(dir.path(), "next panic\n").unwrap();
        assert_eq!(unreported_crash(dir.path()).unwrap(), Some(report));
    }

    #[test]
    fn a_notice_identifies_the_last_report_appended_to_a_daily_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = "2026-11-27T12:00:01Z ERROR sparrow_app::panic_hook: process 1 first\n  first backtrace\n";
        let last = "2026-11-27T18:00:05.202246Z ERROR sparrow_app::panic_hook: process 2 last\n  last backtrace\n";
        std::fs::write(
            dir.path().join("crash-sparrow.2026-11-27.log"),
            format!("{first}{last}"),
        )
        .unwrap();

        let report = unreported_crash(dir.path()).unwrap().unwrap();
        assert_eq!(
            report.notice(),
            "Sparrow crashed at 2026-11-27 18:00:05.202 UTC."
        );
        let entry = report.entry.unwrap();
        assert_eq!(entry.offset, first.len() as u64);
        assert!(entry.header.ends_with("process 2 last"));
        assert_eq!(unreported_crash(dir.path()).unwrap(), None);
    }
}

#[cfg(windows)]
pub(super) fn error_modal(title: &str, msg: &str) -> i32 {
    use std::{ffi::OsStr, iter::once, mem, os::windows::prelude::OsStrExt};

    use windows::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_SYSTEMMODAL, MessageBoxW,
    };

    let result: i32;

    let title = OsStr::new(title)
        .encode_wide()
        .chain(once(0u16))
        .collect::<Vec<u16>>();
    let msg = OsStr::new(msg)
        .encode_wide()
        .chain(once(0u16))
        .collect::<Vec<u16>>();
    unsafe {
        result = MessageBoxW(
            mem::zeroed(),
            msg.as_ptr(),
            title.as_ptr(),
            MB_ICONERROR | MB_SYSTEMMODAL,
        );
    }

    result
}

#[cfg(unix)]
pub fn error_notification(title: &str, msg: &str) {
    let _res = std::process::Command::new("notify-send")
        .args([
            "-a",
            "dev.sparrow.sparrow",
            "-w",
            "-n",
            "dev.sparrow.sparrow",
            "-c",
            "error",
            title,
            msg,
        ])
        .spawn();
}
