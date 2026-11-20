//! Cancellable adb capture. The UI receives bounded batches, never one callback per line.
use super::model::{self, Device};
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub struct Stream(Arc<AtomicBool>);
impl Drop for Stream {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub enum Update {
    Lines(Vec<String>),
    Error(String),
    End,
}
impl Stream {
    pub fn start(
        adb: PathBuf,
        serial: String,
        send: impl Fn(Update) + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        std::thread::spawn(move || {
            let mut child = match Command::new(adb)
                .args([
                    "-s",
                    &serial,
                    "logcat",
                    "-T",
                    "500",
                    "-v",
                    "threadtime",
                    "-v",
                    "year",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(error) => {
                    send(Update::Error(error.to_string()));
                    return;
                }
            };
            let (tx, rx) = crossbeam_channel::bounded(2048);
            let readers = [
                child
                    .stdout
                    .take()
                    .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
                child
                    .stderr
                    .take()
                    .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
            ];
            for (index, pipe) in readers.into_iter().enumerate() {
                if let Some(pipe) = pipe {
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let mut reader = BufReader::new(pipe);
                        let mut bytes = Vec::new();
                        loop {
                            bytes.clear();
                            match reader
                                .by_ref()
                                .take(65537)
                                .read_until(b'\n', &mut bytes)
                            {
                                Ok(0) | Err(_) => break,
                                _ => {}
                            }
                            let line = String::from_utf8_lossy(&bytes)
                                .trim_end_matches(['\r', '\n'])
                                .to_owned();
                            if tx.send((index, line)).is_err() {
                                break;
                            }
                        }
                    });
                }
            }
            drop(tx);
            let mut batch = Vec::new();
            let mut last = Instant::now();
            while !cancelled.load(Ordering::Acquire) {
                if batch.len() >= 256 {
                    if last.elapsed() < Duration::from_millis(50) {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    send(Update::Lines(std::mem::take(&mut batch)));
                    last = Instant::now();
                }
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok((0, line)) => {
                        batch.push(line);
                    }
                    Ok((_, error)) => send(Update::Error(error)),
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                    _ => {}
                }
                if !batch.is_empty() && (last.elapsed() >= Duration::from_millis(50))
                {
                    send(Update::Lines(std::mem::take(&mut batch)));
                    last = Instant::now();
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            drop(rx);
            if !cancelled.load(Ordering::Acquire) {
                if !batch.is_empty() {
                    send(Update::Lines(batch));
                }
                send(Update::End);
            }
        });
        Self(stop)
    }
}
#[derive(Clone, Default)]
pub struct Catalog {
    pub devices: Vec<Device>,
    pub processes: std::collections::HashMap<u32, (String, String)>,
    pub error: String,
}
pub fn discover(serial: &str) -> Catalog {
    let adb = super::super::reference_tools::adb();
    let text = match super::super::reference_tools::output(
        &adb,
        &["devices", "-l"],
        None,
    ) {
        Ok(text) => text,
        Err(error) => {
            return Catalog {
                error,
                ..Default::default()
            };
        }
    };
    let mut devices = model::devices(&text);
    // Static build metadata is cached; process identities are refreshed separately.
    static VERSIONS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, String>>,
    > = std::sync::LazyLock::new(Default::default);
    for device in devices.iter_mut().filter(|d| d.ready) {
        let cached = VERSIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&device.serial)
            .cloned();
        let version = cached.or_else(|| {
            let props = super::super::reference_tools::output(
                &adb,
                &["-s", &device.serial, "shell", "getprop"],
                None,
            )
            .ok()?;
            let property = |key: &str| {
                props.lines().find_map(|line| {
                    line.strip_prefix(&format!("[{key}]: ["))
                        .and_then(|value| value.strip_suffix(']'))
                })
            };
            let version = format!(
                "Android {}, API {}",
                property("ro.build.version.release")?,
                property("ro.build.version.sdk")?
            );
            VERSIONS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(device.serial.clone(), version.clone());
            Some(version)
        });
        if let Some(version) = version {
            device.label = format!("{} {version}", device.label);
        }
    }
    let processes = if devices.iter().any(|d| d.serial == serial && d.ready) {
        super::super::reference_tools::output(
            &adb,
            &["-s", serial, "shell", "ps", "-A", "-o", "PID,NAME"],
            None,
        )
        .map(|text| model::processes(&text))
        .unwrap_or_default()
    } else {
        Default::default()
    };
    Catalog {
        devices,
        processes,
        error: String::new(),
    }
}
#[cfg(all(test, target_os = "linux"))]
mod stream_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn stream_delivers_multiple_batches_and_drop_reaps_process() {
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("adb");
        let pidfile = root.path().join("pid");
        std::fs::write(&program,format!("#!/usr/bin/python3\nimport os,time\nopen({:?},'w').write(str(os.getpid()))\nfor i in range(10000):\n print('2026-11-20 12:04:08.387 12 13 I Test: value '+str(i),flush=True)\n time.sleep(.01)\n",pidfile.to_str().unwrap())).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let stream =
            Stream::start(program, "serial with spaces".into(), move |update| {
                tx.send(update).unwrap();
            });
        for _ in 0..2 {
            let Update::Lines(lines) =
                rx.recv_timeout(Duration::from_secs(3)).unwrap()
            else {
                panic!("Expected logs")
            };
            assert!(!lines.is_empty());
            assert!(lines.iter().all(|line| model::parse(line).is_some()));
        }
        let pid = std::fs::read_to_string(pidfile).unwrap();
        drop(stream);
        for _ in 0..100 {
            if !Path::new(&format!("/proc/{pid}")).exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("Capture process survived cancellation");
    }
}

pub fn java_location(
    message: &str,
    root: Option<&Path>,
) -> Option<crate::editor::location::EditorLocation> {
    static FRAME: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| {
            regex::Regex::new(
                r"(?:at\s+)?([\w.$]+)\(([^():/]+\.(?:java|kt)):(\d+)\)",
            )
            .unwrap()
        });
    let root = root?;
    let frame = FRAME.captures(message)?;
    let class = frame[1].rsplit_once('.')?.0;
    let package = class
        .rsplit_once('.')
        .map(|(p, _)| p)
        .unwrap_or("")
        .replace('.', "/");
    for source in [
        "android/app/src/main/java",
        "android/app/src/main/kotlin",
        "app/src/main/java",
        "app/src/main/kotlin",
        "src/main/java",
        "src/main/kotlin",
    ] {
        let path = root.join(source).join(&package).join(&frame[2]);
        if path.is_file() {
            return Some(crate::editor::location::EditorLocation {
                path,
                position: Some(crate::editor::location::EditorPosition::Line(
                    frame[3].parse::<usize>().ok()?.saturating_sub(1),
                )),
                scroll_offset: None,
                ignore_unconfirmed: false,
                same_editor_tab: false,
            });
        }
    }
    None
}
#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn resolves_workspace_kotlin_stack_frames() {
        let root = tempfile::tempdir().unwrap();
        let source = root
            .path()
            .join("android/app/src/main/kotlin/io/example/MainActivity.kt");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "class MainActivity").unwrap();
        let location = java_location(
            "    at io.example.MainActivity.run(MainActivity.kt:42)",
            Some(root.path()),
        )
        .unwrap();
        assert_eq!(location.path, source);
        assert!(
            java_location("at io.example.Other.run(Other.kt:1)", Some(root.path()))
                .is_none()
        );
    }
}
