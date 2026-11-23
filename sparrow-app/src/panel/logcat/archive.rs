//! Bounded, offline capture import. No commands or external paths in a capture are executed.
use super::{
    lifecycle,
    model::{self, Buffer, Entry},
};
use chrono::Datelike;
use regex::Regex;
use std::{
    io::Read,
    path::Path,
    sync::{Arc, LazyLock},
};
const LIMIT: u64 = 64 * 1024 * 1024;
pub struct Capture {
    pub buffer: Buffer,
    pub filter: String,
}
pub fn read(path: &Path) -> Result<Capture, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Select a regular capture file".into());
    }
    let year = file
        .metadata()
        .ok()
        .and_then(|m| m.created().or_else(|_| m.modified()).ok())
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).year())
        .unwrap_or_else(|| chrono::Local::now().year());
    let mut bytes = Vec::new();
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let index = (0..zip.len())
            .find(|i| {
                zip.by_index(*i).is_ok_and(|e| {
                    !e.is_dir()
                        && e.name().rsplit('/').next().is_some_and(|n| {
                            n.starts_with("bugreport") && n.ends_with(".txt")
                        })
                })
            })
            .ok_or("No bugreport text capture in archive")?;
        let entry = zip.by_index(index).map_err(|e| e.to_string())?;
        if entry.size() > LIMIT {
            return Err("Capture exceeds the 64 MiB import limit".into());
        }
        entry
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
    } else {
        if file.metadata().map_err(|e| e.to_string())?.len() > LIMIT {
            return Err("Capture exceeds the 64 MiB import limit".into());
        }
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
    }
    if bytes.len() as u64 > LIMIT {
        return Err("Capture exceeds the 64 MiB import limit".into());
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "Capture is not UTF-8 text or a supported bugreport archive")?
        .trim_start_matches('\u{feff}');
    if text.trim_start().starts_with('{') {
        json(text)
    } else {
        text_capture(text, year)
    }
}
fn text_capture(text: &str, year: i32) -> Result<Capture, String> {
    static UID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d\d-\d\d \d\d:\d\d:\d\d\.\d+)\s+\w+\s+(\d+\s+\d+\s+[VDIWEAF]\s+.*)$").unwrap()
    });
    static FIREBASE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d\d-\d\d \d\d:\d\d:\d\d\.\d+): ([VDIWEAF])/(.*?)\(\s*(\d+)\): (.*)$").unwrap()
    });
    static STUDIO: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+)\s+(\d+)-(\d+)\s+(\S+)\s+(\S+)\s+([VDIWEAF])\s+(.*)$").unwrap()
    });
    static MARKER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"PROCESS (STARTED|ENDED) \((\d+)\) for package ([\w.]+)")
            .unwrap()
    });
    let mut buffer = Buffer::default();
    let mut previous = None::<Entry>;
    let bugreport = text.contains("------ SYSTEM LOG ");
    let mut in_logs = !bugreport;
    for (index, line) in text.lines().enumerate() {
        if bugreport {
            if line.starts_with("------ SYSTEM LOG ") {
                in_logs = true;
                continue;
            }
            if line.contains(" was the duration of 'SYSTEM LOG' ") {
                in_logs = false;
                continue;
            }
            if !in_logs {
                continue;
            }
        }
        if line.trim().is_empty() {
            continue;
        }
        let normalized = if let Some(c) = UID.captures(line) {
            format!("{year}-{} {}", &c[1], &c[2])
        } else if let Some(c) = FIREBASE.captures(line) {
            format!(
                "{year}-{} {} 0 {} {}: {}",
                &c[1], &c[4], &c[2], &c[3], &c[5]
            )
        } else if line.len() >= 6 && line.as_bytes().get(2) == Some(&b'-') {
            format!("{year}-{line}")
        } else {
            line.into()
        };
        let entry = if let Some(e) = model::parse(&normalized) {
            previous = Some(e.clone());
            e
        } else if let Some(c) = STUDIO.captures(line) {
            let mut e = model::parse(&format!(
                "{} {} {} {} {}: {}",
                &c[1], &c[2], &c[3], &c[6], &c[4], &c[7]
            ))
            .ok_or("Invalid formatted record")?;
            e.package = c[5].into();
            e.process = c[5].into();
            e.raw = line.into();
            previous = Some(e.clone());
            e
        } else if let Some(c) = MARKER.captures(line) {
            lifecycle::marker(
                c[2].parse().map_err(|_| "Invalid marker PID")?,
                &c[3],
                &c[3],
                &c[1] == "STARTED",
            )
        } else if line.starts_with("---------") {
            Entry {
                message: line.into(),
                raw: line.into(),
                marker: true,
                ..Default::default()
            }
        } else if (line.starts_with(char::is_whitespace)
            || line.starts_with("Caused by:")
            || line.starts_with("... "))
            && previous.is_some()
        {
            let mut e = previous.clone().unwrap();
            e.message = line.into();
            e.raw = line.into();
            e
        } else {
            return Err(format!(
                "Unsupported or malformed capture record at line {}",
                index + 1
            ));
        };
        buffer.push(entry);
    }
    if buffer.entries.is_empty() {
        return Err("Capture contains no supported Logcat records".into());
    }
    Ok(Capture {
        buffer,
        filter: String::new(),
    })
}
fn json(text: &str) -> Result<Capture, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| format!("Invalid capture JSON: {e}"))?;
    let messages = value["logcatMessages"]
        .as_array()
        .ok_or("JSON capture has no logcatMessages array")?;
    let mut buffer = Buffer::default();
    for (i, record) in messages.iter().enumerate() {
        let header = &record["header"];
        let message = record["message"]
            .as_str()
            .ok_or_else(|| format!("Missing message in record {}", i + 1))?;
        let level = header["logLevel"].as_str().ok_or("Missing log level")?;
        let n = model::priority(level).ok_or("Unknown capture log level")?;
        let level = ['V', 'D', 'I', 'W', 'E', 'F'][n as usize];
        let pid = header["pid"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("Invalid process ID")?;
        let tid = header["tid"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("Invalid thread ID")?;
        let time = &header["timestamp"];
        let instant = if let Some(time) = time.as_str() {
            chrono::DateTime::parse_from_rfc3339(time)
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc))
        } else {
            time["seconds"]
                .as_i64()
                .or_else(|| time["epochSecond"].as_i64())
                .and_then(|s| {
                    chrono::DateTime::from_timestamp(
                        s,
                        time["nanos"]
                            .as_u64()
                            .or_else(|| time["nano"].as_u64())
                            .unwrap_or(0) as u32,
                    )
                })
        }
        .ok_or("Invalid capture timestamp")?;
        let timestamp = instant
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S%.3f")
            .to_string();
        let tag = header["tag"].as_str().unwrap_or("");
        for line in
            message
                .lines()
                .chain(if message.is_empty() { Some("") } else { None })
        {
            buffer.push(Entry {
                timestamp: timestamp.clone(),
                time: instant.timestamp(),
                pid,
                tid,
                level,
                tag: tag.into(),
                package: header["applicationId"].as_str().unwrap_or("").into(),
                process: header["processName"].as_str().unwrap_or("").into(),
                message: line.into(),
                raw: format!("{timestamp} {pid} {tid} {level} {tag}: {line}"),
                marker: pid == 0 || tag == "Sparrow.Process",
                ..Default::default()
            });
        }
    }
    let packages = value["metadata"]["projectApplicationIds"]
        .as_array()
        .map(|p| {
            p.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut filter = value["metadata"]["filter"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if filter.contains("package:mine") {
        let explicit = packages
            .iter()
            .map(|p| {
                format!(
                    "package:\"{}\"",
                    p.replace('\\', "\\\\").replace('"', "\\\"")
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        filter = filter.replace(
            "package:mine",
            if explicit.is_empty() { "" } else { &explicit },
        );
        if !explicit.is_empty() {
            filter = value["metadata"]["filter"]
                .as_str()
                .unwrap_or("")
                .replace("package:mine", &format!("({explicit})"));
        }
    }
    Ok(Capture { buffer, filter })
}
pub fn encode(entries: &[Arc<Entry>]) -> Result<String, String> {
    let messages=entries.iter().map(|e|serde_json::json!({"header":{"logLevel":match e.level{'V'=>"VERBOSE",'D'=>"DEBUG",'W'=>"WARN",'E'=>"ERROR",'F'|'A'=>"ASSERT",_=>"INFO"},"pid":e.pid,"tid":e.tid,"applicationId":e.package,"processName":e.process,"tag":e.tag,"timestamp":{"seconds":e.time,"nanos":e.timestamp.rsplit_once('.').map(|(_,f)|format!("{f:0<9}").parse::<u32>().unwrap_or(0)).unwrap_or(0)}},"message":e.message})).collect::<Vec<_>>();
    serde_json::to_string_pretty(
        &serde_json::json!({"metadata":null,"logcatMessages":messages}),
    )
    .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_legacy_text_and_continuations() {
        let c=text_capture("11-20 12:00:00.123 42 43 E Test: Boom\n    at io.app.Main.run(Main.java:4)\n",2026).unwrap();
        assert_eq!(c.buffer.entries.len(), 2);
        assert_eq!(c.buffer.entries[1].pid, 42);
        assert!(text_capture("not a log", 2026).is_err());
    }
    #[test]
    fn json_roundtrip_preserves_identity_and_unicode() {
        let e = Arc::new(Entry {
            timestamp: "2026-11-20 12:00:00.123".into(),
            time: 1795161600,
            pid: 42,
            tid: 43,
            level: 'E',
            package: "io.app".into(),
            process: "io.app:worker".into(),
            tag: "Test".into(),
            message: "世界".into(),
            ..Default::default()
        });
        let c = json(&encode(&[e]).unwrap()).unwrap();
        let e = c.buffer.entries.front().unwrap();
        assert_eq!(e.package, "io.app");
        assert_eq!(e.message, "世界");
        assert!(e.timestamp.ends_with(".123"));
        assert!(json("{}").is_err());
    }
    #[test]
    fn metadata_mine_is_bound_to_capture_project() {
        let c=json(r#"{"metadata":{"filter":"package:mine level:ERROR","projectApplicationIds":["io.old"]},"logcatMessages":[]}"#).unwrap();
        assert_eq!(c.filter, "(package:\"io.old\") level:ERROR");
    }
    #[test]
    fn imports_zip_without_extracting_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("report.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("bugreport-test.txt", zip::write::FileOptions::default())
            .unwrap();
        use std::io::Write;
        zip.write_all(b"11-20 12:00:00.123 42 43 I Test: captured\n")
            .unwrap();
        zip.finish().unwrap();
        assert_eq!(read(&path).unwrap().buffer.entries.len(), 1);
    }
}
