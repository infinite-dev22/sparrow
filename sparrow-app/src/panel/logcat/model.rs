use chrono::{NaiveDateTime, TimeZone};
use regex::Regex;
use std::{
    collections::{HashMap, VecDeque},
    sync::LazyLock,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub timestamp: String,
    pub time: i64,
    pub pid: u32,
    pub tid: u32,
    pub tag: String,
    pub package: String,
    pub process: String,
    pub level: char,
    pub message: String,
    pub raw: String,
    pub marker: bool,
}
pub fn priority(level: &str) -> Option<u8> {
    match level.to_ascii_uppercase().as_str() {
        "V" | "VERBOSE" => Some(0),
        "D" | "DEBUG" => Some(1),
        "I" | "INFO" => Some(2),
        "W" | "WARN" | "WARNING" => Some(3),
        "E" | "ERROR" => Some(4),
        "A" | "F" | "ASSERT" | "FATAL" => Some(5),
        _ => None,
    }
}
pub fn parse(line: &str) -> Option<Entry> {
    static HEADER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+)\s+(\d+)\s+(\d+)\s+([VDIWEAF])\s+(.*?)\s*:\s?(.*)$").unwrap()
    });
    let c = HEADER.captures(line)?;
    let time = NaiveDateTime::parse_from_str(&c[1], "%Y-%m-%d %H:%M:%S%.f")
        .ok()
        .and_then(|time| chrono::Local.from_local_datetime(&time).earliest())
        .map(|time| time.timestamp())
        .unwrap_or(0);
    Some(Entry {
        timestamp: c[1].into(),
        time,
        pid: c[2].parse().ok()?,
        tid: c[3].parse().ok()?,
        level: c[4].chars().next()?,
        tag: c[5].trim().into(),
        message: c[6].into(),
        raw: line.into(),
        ..Default::default()
    })
}
/// Bound memory by bytes as well as entry count, including a single large line.
#[derive(Clone)]
pub struct Buffer {
    pub entries: VecDeque<std::sync::Arc<Entry>>,
    next: u64,
    bytes: usize,
    pub dropped: u64,
}
impl Default for Buffer {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            next: 1,
            bytes: 0,
            dropped: 0,
        }
    }
}
impl Buffer {
    pub fn push(&mut self, mut entry: Entry) {
        const MAX_BYTES: usize = 8 * 1024 * 1024;
        if entry.raw.len() > 65536 {
            entry.raw = truncate(&entry.raw, 65536);
            entry.message = truncate(&entry.message, 65536);
        }
        entry.id = self.next;
        self.next = self.next.wrapping_add(1);
        self.bytes += entry.raw.len()
            + entry.message.len()
            + entry.package.len()
            + entry.process.len()
            + entry.tag.len()
            + entry.timestamp.len()
            + 128;
        self.entries.push_back(std::sync::Arc::new(entry));
        while self.entries.len() > 20000 || self.bytes > MAX_BYTES {
            if let Some(e) = self.entries.pop_front() {
                self.bytes = self.bytes.saturating_sub(
                    e.raw.len()
                        + e.message.len()
                        + e.package.len()
                        + e.process.len()
                        + e.tag.len()
                        + e.timestamp.len()
                        + 128,
                );
                self.dropped += 1;
            } else {
                break;
            }
        }
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.dropped = 0;
    }
}
fn truncate(value: &str, max: usize) -> String {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub label: String,
    pub ready: bool,
}
impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}
pub fn devices(text: &str) -> Vec<Device> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let serial = words.next()?;
            let status = words.next()?;
            if !matches!(status, "device" | "offline" | "unauthorized") {
                return None;
            }
            let model = words
                .find_map(|s| s.strip_prefix("model:"))
                .unwrap_or(serial)
                .replace('_', " ");
            Some(Device {
                serial: serial.into(),
                label: format!(
                    "{model} ({serial}){}",
                    if status == "device" {
                        String::new()
                    } else {
                        format!(" — {status}")
                    }
                ),
                ready: status == "device",
            })
        })
        .collect()
}
pub fn processes(text: &str) -> HashMap<u32, (String, String)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let process = parts.next()?.to_string();
            let package = process.split(':').next().unwrap_or(&process).to_string();
            Some((pid, (package, process)))
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_headers_without_losing_colons_or_unicode() {
        let e = parse("2026-11-20 12:04:08.387 2446 3533 W My Tag: hello: 世界")
            .unwrap();
        assert_eq!((e.pid, e.tid, e.level), (2446, 3533, 'W'));
        assert_eq!(e.message, "hello: 世界");
        assert_eq!(e.tag, "My Tag");
        assert!(parse("    at example.java:42").is_none());
    }
    #[test]
    fn keeps_unavailable_devices_visible() {
        let d = devices(
            "List of devices attached\na device model:Pixel_9\nb unauthorized\nc offline\n",
        );
        assert_eq!(d.len(), 3);
        assert!(!d[1].ready);
    }
    #[test]
    fn buffer_is_bounded_and_unicode_safe() {
        let mut b = Buffer::default();
        for _ in 0..21000 {
            b.push(Entry {
                raw: "line".into(),
                ..Default::default()
            });
        }
        assert_eq!(b.entries.len(), 20000);
        assert_eq!(b.dropped, 1000);
        b.push(Entry {
            raw: "界".repeat(30000),
            message: "界".repeat(30000),
            ..Default::default()
        });
        assert!(b.entries.back().unwrap().raw.len() <= 65536);
        b.clear();
        assert!(b.entries.is_empty());
    }
}
