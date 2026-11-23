//! Owns the Flutter web server or official Widget Previewer process.
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    App,
    Widgets,
}

#[derive(Clone)]
pub(super) enum Event {
    Ready(String),
    Failed(String),
}

pub(super) fn start(
    flutter: PathBuf,
    root: PathBuf,
    mode: Mode,
) -> (Sender<()>, std::sync::mpsc::Receiver<Event>) {
    let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Err(error) = run(&flutter, &root, mode, stop_rx, &tx) {
            let _ = tx.send(Event::Failed(error));
        }
    });
    (stop_tx, rx)
}

fn run(
    flutter: &Path,
    root: &Path,
    mode: Mode,
    cancel: Receiver<()>,
    output: &std::sync::mpsc::Sender<Event>,
) -> Result<(), String> {
    let mut command = Command::new(flutter);
    match mode {
        Mode::App => command.args([
            "run",
            "-d",
            "web-server",
            "--web-hostname",
            "127.0.0.1",
            "--web-port",
            "0",
            "--machine",
        ]),
        Mode::Widgets => {
            command.args(["widget-preview", "start", "--web-server", "--machine"])
        }
    };
    let child = group(&mut command)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start Flutter: {error}"))?;
    let mut process = Managed(child);
    let (lines, log) = crossbeam_channel::unbounded();
    for stream in [
        Box::new(process.0.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
        Box::new(process.0.stderr.take().unwrap()),
    ] {
        let lines = lines.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(lines);
    let mut ready = false;
    let mut last = String::new();
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match cancel.try_recv() {
            Ok(()) | Err(crossbeam_channel::TryRecvError::Disconnected) => {
                return Ok(());
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
        match log.recv_timeout(Duration::from_millis(150)) {
            Ok(line) => {
                if !ready && let Some(url) = launch_url(&line) {
                    output.send(Event::Ready(url)).map_err(|e| e.to_string())?;
                    ready = true;
                }
                if !line.trim().is_empty() {
                    last = line;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(format!("Flutter preview server exited. {last}"));
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        if !ready && Instant::now() >= deadline {
            return Err(format!(
                "Flutter preview server did not become ready. {last}"
            ));
        }
    }
}

fn launch_url(line: &str) -> Option<String> {
    let values: Vec<Value> = serde_json::from_str(line.trim()).ok()?;
    let raw = values.iter().find_map(|value| {
        let event = value["event"].as_str()?;
        matches!(
            event,
            "app.webLaunchUrl"
                | "widget_preview.app.webLaunchUrl"
                | "widget_preview.started"
        )
        .then(|| value["params"]["url"].as_str())
        .flatten()
    })?;
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != "http" {
        return None;
    }
    match url.host_str()? {
        "localhost" | "127.0.0.1" | "[::1]" => Some(url.into()),
        _ => None,
    }
}

pub(super) fn group(command: &mut Command) -> &mut Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

pub(super) fn stop(child: &mut Child) {
    #[cfg(unix)]
    let command = (
        "kill",
        vec![
            "-TERM".to_owned(),
            "--".to_owned(),
            format!("-{}", child.id()),
        ],
    );
    #[cfg(windows)]
    let command = (
        "taskkill",
        vec![
            "/PID".to_owned(),
            child.id().to_string(),
            "/T".to_owned(),
            "/F".to_owned(),
        ],
    );
    #[cfg(any(unix, windows))]
    let _ = Command::new(command.0)
        .args(command.1)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

struct Managed(Child);
impl Drop for Managed {
    fn drop(&mut self) {
        stop(&mut self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_flutter_local_launch_event() {
        assert_eq!(launch_url(r#"[{"event":"app.webLaunchUrl","params":{"url":"http://localhost:7500"}}]"#).as_deref(), Some("http://localhost:7500/"));
        assert!(launch_url(r#"[{"event":"app.webLaunchUrl","params":{"url":"http://example.com:7500"}}]"#).is_none());
        assert!(launch_url(r#"[{"event":"app.debugPort","params":{"url":"http://localhost:7500"}}]"#).is_none());
    }
}
