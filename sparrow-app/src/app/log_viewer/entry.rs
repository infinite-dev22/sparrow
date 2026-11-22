//! Structured fields from Sparrow's current and legacy log formats.

use crate::config::color::SparrowColor;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Level {
    Other,
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub(super) const ALL: [Self; 6] = [
        Self::Other,
        Self::Trace,
        Self::Debug,
        Self::Info,
        Self::Warn,
        Self::Error,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Other => "Unclassified",
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub(super) fn color(self) -> &'static str {
        match self {
            Self::Error => SparrowColor::SPARROW_ERROR,
            Self::Warn => SparrowColor::SPARROW_WARN,
            Self::Info => SparrowColor::EDITOR_LINK,
            _ => SparrowColor::PANEL_FOREGROUND_DIM,
        }
    }
}

#[derive(PartialEq)]
pub(super) struct Entry {
    pub time: String,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub level: Level,
    pub summary: String,
    pub component: Option<String>,
    pub fields: Vec<(&'static str, String)>,
    pub raw: String,
    pub searchable: String,
}

impl Entry {
    pub fn timestamp_label(&self) -> Option<String> {
        self.timestamp
            .map(|time| time.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string())
    }

    pub fn overview(&self) -> (&'static str, Option<&'static str>) {
        let message = self.summary.as_str();
        if self.component.as_deref() == Some("sparrow_app::panic_hook") {
            return (
                "Sparrow stopped unexpectedly",
                Some(
                    "The report records where Sparrow stopped. The Technical tab includes the backtrace when one was recorded.",
                ),
            );
        }
        if message.starts_with("Terminal launch failed") {
            return (
                "The terminal could not start",
                Some(
                    "Check that the shell command in the terminal profile exists and can be launched.",
                ),
            );
        }
        match self.level {
            Level::Error => ("An error was reported", None),
            Level::Warn => ("A warning was reported", None),
            Level::Info => ("Information", None),
            Level::Trace | Level::Debug => ("Development diagnostics", None),
            Level::Other => ("Log output", None),
        }
    }

    pub fn continuation(&self) -> Option<&str> {
        self.raw
            .split_once('\n')
            .map(|(_, continuation)| continuation)
    }
}

fn source_location(text: &str) -> bool {
    let Some((prefix, line)) = text.rsplit_once(':') else {
        return false;
    };
    if line.parse::<u32>().is_err() {
        return false;
    }
    let path = prefix
        .rsplit_once(':')
        .filter(|(_, line)| line.parse::<u32>().is_ok())
        .map_or(prefix, |(path, _)| path);
    path.contains(['/', '\\']) || path.ends_with(".rs")
}

fn trace_fields(prefix: &str, entry: &mut Entry) {
    if let Some((thread, tail)) = prefix.split_once("ThreadId(")
        && let Some((id, component)) = tail.split_once(')')
    {
        if !thread.trim().is_empty() {
            entry.fields.push(("Thread", thread.trim().into()));
        }
        entry.fields.push(("Thread ID", id.into()));
        entry.component = Some(component.trim().into());
        return;
    }
    if let Some((thread, component)) = prefix.rsplit_once(' ') {
        entry.fields.push(("Thread", thread.into()));
        entry.component = Some(component.into());
    } else {
        entry.component = Some(prefix.into());
    }
}

fn crash_fields<'a>(text: &'a str, entry: &mut Entry) -> Option<&'a str> {
    let text = text.strip_prefix("process ")?;
    let (process, remainder) = text.split_once(" (")?;
    let (version, remainder) = remainder.split_once(") thread ")?;
    let (thread, remainder) = remainder.split_once(" panicked at ")?;
    let (location, message) = remainder.split_once(": ")?;
    entry.fields.extend([
        ("Process ID", process.into()),
        ("Sparrow version", version.into()),
        ("Thread", thread.into()),
        ("Source", location.into()),
    ]);
    Some(message)
}

fn parse_header(line: &str) -> Option<Entry> {
    let (timestamp, rest) = line.split_once(' ')?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .to_utc();
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
    let mut entry = Entry {
        time: timestamp.format("%H:%M:%S%.3f").to_string(),
        timestamp: Some(timestamp),
        level,
        summary: String::new(),
        component: None,
        fields: Vec::new(),
        raw: line.into(),
        searchable: String::new(),
    };
    let mut summary = message;
    if let Some((prefix, body)) = message.split_once(": ") {
        trace_fields(prefix, &mut entry);
        summary = body;
        if entry.component.as_deref() == Some("sparrow_app::panic_hook")
            && let Some(payload) = crash_fields(body, &mut entry)
        {
            summary = payload;
        } else if let Some((location, message)) = body.split_once(": ")
            && source_location(location)
        {
            entry.fields.push(("Source", location.into()));
            summary = message;
        }
    }
    entry.summary = summary.into();
    Some(entry)
}

pub(super) fn parse_entries(input: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in input.lines() {
        // Keep text and clipboard usable when a tool emits control codes.
        let line: String = line
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        if let Some(entry) = parse_header(&line) {
            entries.push(entry);
        } else if let Some(entry) = entries.last_mut() {
            entry.raw.push('\n');
            entry.raw.push_str(&line);
        } else {
            entries.push(Entry {
                time: String::new(),
                timestamp: None,
                level: Level::Other,
                summary: line.clone(),
                component: None,
                fields: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_diagnostics_from_a_full_panic_and_keeps_the_backtrace() {
        let input = "2026-12-04T18:00:06.677927372+00:00 ERROR sparrow_app::panic_hook: process 3 (0.4.6+Debug) thread main panicked at C:\\dev\\app.rs:926:13: a failure: details\n   0: frame";
        let mut entries = parse_entries(input);
        let entry = entries.pop().unwrap();
        assert_eq!(
            entry.timestamp_label().as_deref(),
            Some("2026-12-04 18:00:06.677 UTC")
        );
        assert_eq!(entry.summary, "a failure: details");
        assert_eq!(entry.component.as_deref(), Some("sparrow_app::panic_hook"));
        assert_eq!(
            entry.fields,
            [
                ("Process ID", "3".into()),
                ("Sparrow version", "0.4.6+Debug".into()),
                ("Thread", "main".into()),
                ("Source", "C:\\dev\\app.rs:926:13".into()),
            ]
        );
        assert_eq!(entry.continuation(), Some("   0: frame"));
    }

    #[test]
    fn extracts_tracing_fields_from_named_threads_and_source_locations() {
        let entries = parse_entries(
            "2026-12-04T18:00:06Z WARN Worker 2 ThreadId(09) sparrow_core::language: src/language.rs:811: Grammar unavailable",
        );
        let entry = &entries[0];
        assert_eq!(entry.summary, "Grammar unavailable");
        assert_eq!(entry.component.as_deref(), Some("sparrow_core::language"));
        assert_eq!(
            entry.fields,
            [
                ("Thread", "Worker 2".into()),
                ("Thread ID", "09".into()),
                ("Source", "src/language.rs:811".into()),
            ]
        );
    }
}
