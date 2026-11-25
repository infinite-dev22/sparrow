//! Owns the Flutter web server or official Widget Previewer process.
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};
use notify::Watcher;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    App,
    Widgets,
}

#[derive(Clone)]
pub(super) enum Event {
    Ready {
        url: String,
        app_id: Option<String>,
    },
    Failed(String),
    Notice(String),
    RuntimeVersion(String),
    OperationStarted {
        operation: Operation,
        automatic: bool,
    },
    OperationFinished {
        operation: Operation,
        automatic: bool,
        result: Result<(), String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ServerCommand {
    Stop,
    Reload,
    Restart,
    SetAutoReload(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Reload,
    Restart,
}

impl Operation {
    fn is_full_restart(self) -> bool {
        self == Self::Restart
    }
}

enum WatchEvent {
    Changed,
    Error(String),
}

pub(super) fn start(
    flutter: PathBuf,
    root: PathBuf,
    mode: Mode,
    auto_reload_on_save: bool,
) -> (Sender<ServerCommand>, std::sync::mpsc::Receiver<Event>) {
    let (command_tx, command_rx) = crossbeam_channel::unbounded();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Err(error) =
            run(&flutter, &root, mode, auto_reload_on_save, command_rx, &tx)
        {
            let _ = tx.send(Event::Failed(error));
        }
    });
    (command_tx, rx)
}

fn run(
    flutter: &Path,
    root: &Path,
    mode: Mode,
    mut auto_reload_on_save: bool,
    commands: Receiver<ServerCommand>,
    output: &std::sync::mpsc::Sender<Event>,
) -> Result<(), String> {
    if let Some(version) = flutter_runtime_version(flutter) {
        output
            .send(Event::RuntimeVersion(version))
            .map_err(|error| error.to_string())?;
    }
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
    let mut stdin = process
        .0
        .stdin
        .take()
        .ok_or("Could not connect to Flutter's machine protocol.")?;
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
    let (watch_tx, watch_events) = crossbeam_channel::unbounded();
    let mut _watcher = None;
    if mode == Mode::App {
        match watch_sources(root, watch_tx.clone()) {
            Ok(watcher) => _watcher = Some(watcher),
            Err(error) => output
                .send(Event::Notice(format!(
                    "Automatic reload is unavailable: {error}"
                )))
                .map_err(|error| error.to_string())?,
        }
    }
    drop(watch_tx);
    let mut ready = false;
    let mut app_id = None;
    let mut pending: Option<(String, Operation, Instant, bool)> = None;
    let mut auto_reload_at = None;
    let mut request_id = 0;
    let mut last = String::new();
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match commands.try_recv() {
            Ok(ServerCommand::Stop)
            | Err(crossbeam_channel::TryRecvError::Disconnected) => {
                return Ok(());
            }
            Ok(ServerCommand::SetAutoReload(enabled)) => {
                auto_reload_on_save = enabled;
                auto_reload_at = None;
                while watch_events.try_recv().is_ok() {}
            }
            Ok(command @ (ServerCommand::Reload | ServerCommand::Restart)) => {
                let operation = match command {
                    ServerCommand::Reload => Operation::Reload,
                    ServerCommand::Restart => Operation::Restart,
                    _ => unreachable!(),
                };
                auto_reload_at = None;
                let result = if mode != Mode::App {
                    Err("Hot reload is only available for Flutter app previews."
                        .into())
                } else if pending.is_some() {
                    Err("A Flutter reload is already in progress.".into())
                } else if let Some(app_id) = app_id.as_deref() {
                    match start_operation(
                        &mut stdin,
                        &mut request_id,
                        app_id,
                        operation,
                        false,
                        output,
                    ) {
                        Ok(operation) => {
                            pending = Some(operation);
                            continue;
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    Err("Flutter has not reported a running app yet.".into())
                };
                output
                    .send(Event::OperationFinished {
                        operation,
                        automatic: false,
                        result,
                    })
                    .map_err(|error| error.to_string())?;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
        while let Ok(event) = watch_events.try_recv() {
            match event {
                WatchEvent::Changed if auto_reload_on_save => {
                    auto_reload_at =
                        Some(Instant::now() + Duration::from_millis(350));
                }
                WatchEvent::Error(error) => output
                    .send(Event::Notice(format!("File watcher: {error}")))
                    .map_err(|error| error.to_string())?,
                WatchEvent::Changed => {}
            }
        }
        match log.recv_timeout(Duration::from_millis(150)) {
            Ok(line) => {
                if let Some(id) = machine_app_id(&line) {
                    app_id = Some(id);
                }
                if !ready && let Some(url) = launch_url(&line) {
                    output
                        .send(Event::Ready {
                            url,
                            app_id: app_id.clone(),
                        })
                        .map_err(|error| error.to_string())?;
                    ready = true;
                }
                if let Some((id, operation, _, automatic)) = &pending
                    && let Some(result) = machine_response(&line, id)
                {
                    let operation = *operation;
                    let automatic = *automatic;
                    pending = None;
                    output
                        .send(Event::OperationFinished {
                            operation,
                            automatic,
                            result,
                        })
                        .map_err(|error| error.to_string())?;
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
        if let Some((_, operation, timeout, automatic)) = pending
            && Instant::now() >= timeout
        {
            pending = None;
            output
                .send(Event::OperationFinished {
                    operation,
                    automatic,
                    result: Err(
                        "Flutter did not confirm the requested reload.".into()
                    ),
                })
                .map_err(|error| error.to_string())?;
        }
        if auto_reload_on_save
            && auto_reload_at.is_some_and(|scheduled| Instant::now() >= scheduled)
        {
            auto_reload_at = None;
            if pending.is_some() {
                auto_reload_at = Some(Instant::now() + Duration::from_millis(350));
            } else if let Some(app_id) = app_id.as_deref() {
                match start_operation(
                    &mut stdin,
                    &mut request_id,
                    app_id,
                    Operation::Reload,
                    true,
                    output,
                ) {
                    Ok(operation) => pending = Some(operation),
                    Err(error) => output
                        .send(Event::OperationFinished {
                            operation: Operation::Reload,
                            automatic: true,
                            result: Err(error),
                        })
                        .map_err(|error| error.to_string())?,
                }
            }
        }
    }
}

fn start_operation(
    stdin: &mut impl Write,
    request_id: &mut u64,
    app_id: &str,
    operation: Operation,
    automatic: bool,
    output: &std::sync::mpsc::Sender<Event>,
) -> Result<(String, Operation, Instant, bool), String> {
    let id = format!("sparrow-preview-{}", *request_id);
    *request_id += 1;
    write_request(stdin, &id, app_id, operation)?;
    output
        .send(Event::OperationStarted {
            operation,
            automatic,
        })
        .map_err(|error| error.to_string())?;
    Ok((
        id,
        operation,
        Instant::now() + Duration::from_secs(90),
        automatic,
    ))
}

fn flutter_runtime_version(flutter: &Path) -> Option<String> {
    let output = Command::new(flutter)
        .args(["--version", "--machine", "--no-version-check"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version: Value = serde_json::from_slice(&output.stdout).ok()?;
    let framework = version["frameworkVersion"].as_str()?;
    let channel = version["channel"].as_str().unwrap_or("unknown channel");
    let dart = version["dartSdkVersion"].as_str().unwrap_or("unknown Dart");
    Some(format!("Flutter {framework} ({channel}), Dart {dart}"))
}

fn watch_sources(
    root: &Path,
    events: Sender<WatchEvent>,
) -> Result<notify::RecommendedWatcher, String> {
    let watched_root = root.to_path_buf();
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            match result {
                Ok(event) if reloadable_event(&watched_root, &event) => {
                    let _ = events.send(WatchEvent::Changed);
                }
                Err(error) => {
                    let _ = events.send(WatchEvent::Error(error.to_string()));
                }
                _ => {}
            }
        })
        .map_err(|error| error.to_string())?;
    watcher
        .watch(root, notify::RecursiveMode::Recursive)
        .map_err(|error| error.to_string())?;
    Ok(watcher)
}

fn reloadable_event(root: &Path, event: &notify::Event) -> bool {
    matches!(
        &event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Modify(_)
            | notify::EventKind::Remove(_)
    ) && event.paths.iter().any(|path| reloadable_path(root, path))
}

fn reloadable_path(root: &Path, path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "dart")
        && path.strip_prefix(root).ok().is_some_and(|relative| {
            !relative.components().any(|component| {
                matches!(
                    component.as_os_str().to_str(),
                    Some(".git" | ".dart_tool" | "build")
                )
            })
        })
}

fn write_request(
    stdin: &mut impl Write,
    id: &str,
    app_id: &str,
    operation: Operation,
) -> Result<(), String> {
    let request = json!([{
        "id": id,
        "method": "app.restart",
        "params": {
            "appId": app_id,
            "fullRestart": operation.is_full_restart(),
            "pause": false,
            "reason": "Sparrow Mobile Preview",
        }
    }]);
    writeln!(stdin, "{request}")
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("Could not send Flutter reload command: {error}"))
}

fn machine_values(line: &str) -> Option<Vec<Value>> {
    serde_json::from_str(line.trim()).ok()
}

fn machine_app_id(line: &str) -> Option<String> {
    machine_values(line)?.iter().find_map(|value| {
        (value["event"].as_str() == Some("app.start"))
            .then(|| value["params"]["appId"].as_str())
            .flatten()
            .map(str::to_owned)
    })
}

fn machine_response(line: &str, id: &str) -> Option<Result<(), String>> {
    let response = machine_values(line)?
        .into_iter()
        .find(|value| value["id"].as_str() == Some(id))?;
    if let Some(error) = response.get("error") {
        return Some(Err(error["message"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string())));
    }
    let result = &response["result"];
    let code = result["code"].as_i64().unwrap_or(0);
    if code == 0 {
        Some(Ok(()))
    } else {
        Some(Err(result["message"]
            .as_str()
            .filter(|message| !message.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!("Flutter returned operation code {code}.")
            })))
    }
}

fn launch_url(line: &str) -> Option<String> {
    let values = machine_values(line)?;
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
        assert_eq!(
            launch_url(
                r#"[{"event":"app.webLaunchUrl","params":{"url":"http://127.0.0.1:34993","launched":false}}]"#
            )
            .as_deref(),
            Some("http://127.0.0.1:34993/")
        );
        assert!(launch_url(r#"[{"event":"app.webLaunchUrl","params":{"url":"http://example.com:7500"}}]"#).is_none());
        assert!(launch_url(r#"[{"event":"app.debugPort","params":{"url":"http://localhost:7500"}}]"#).is_none());
    }

    #[test]
    fn restart_request_uses_flutter_machine_protocol_framing() {
        let mut request = Vec::new();
        write_request(&mut request, "request-1", "app-1", Operation::Restart)
            .unwrap();
        let messages: Vec<Value> = serde_json::from_slice(&request).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["id"], "request-1");
        assert_eq!(messages[0]["method"], "app.restart");
        assert_eq!(messages[0]["params"]["appId"], "app-1");
        assert_eq!(messages[0]["params"]["fullRestart"], true);
        assert_eq!(messages[0]["params"]["pause"], false);

        let mut request = Vec::new();
        write_request(&mut request, "request-2", "app-1", Operation::Reload)
            .unwrap();
        let messages: Vec<Value> = serde_json::from_slice(&request).unwrap();
        assert_eq!(messages[0]["params"]["fullRestart"], false);
    }

    #[test]
    fn parses_machine_app_id_and_operation_result() {
        let start = r#"[{"event":"app.start","params":{"appId":"app-1"}}]"#;
        let response = r#"[{"id":"request-1","result":{"code":0,"message":""}}]"#;
        let failure =
            r#"[{"id":"request-2","result":{"code":1,"message":"compile failed"}}]"#;

        assert_eq!(machine_app_id(start).as_deref(), Some("app-1"));
        assert_eq!(machine_response(response, "request-1"), Some(Ok(())));
        assert_eq!(
            machine_response(failure, "request-2"),
            Some(Err("compile failed".into()))
        );
    }

    #[test]
    fn reload_watcher_ignores_generated_and_non_dart_files() {
        let root = Path::new("/workspace/flutter_app");

        assert!(reloadable_path(root, &root.join("lib/main.dart")));
        assert!(reloadable_path(root, &root.join("test/widget_test.dart")));
        assert!(!reloadable_path(root, &root.join("build/web/main.dart")));
        assert!(!reloadable_path(
            root,
            &root.join(".dart_tool/package.dart")
        ));
        assert!(!reloadable_path(root, &root.join("web/main.js")));
    }

    #[test]
    #[ignore = "requires SPARROW_PREVIEW_TEST_PROJECT and Flutter on PATH"]
    fn dart_save_triggers_hot_reload_and_machine_protocol_supports_restart() {
        let root = PathBuf::from(
            std::env::var_os("SPARROW_PREVIEW_TEST_PROJECT")
                .expect("set to an isolated disposable Flutter project"),
        );
        let flutter = std::env::var_os("SPARROW_FLUTTER_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| "flutter".into());
        let source = root.join("lib/main.dart");
        let original = std::fs::read_to_string(&source).unwrap();
        let (commands, updates) = start(flutter, root, Mode::App, true);
        let result = (|| {
            loop {
                match updates.recv_timeout(Duration::from_secs(180)) {
                    Ok(Event::Ready {
                        app_id: Some(_), ..
                    }) => break,
                    Ok(Event::Failed(error)) => return Err(error),
                    Ok(_) => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
            std::fs::write(
                &source,
                format!("{original}\n// Sparrow automatic reload verification\n"),
            )
            .map_err(|error| error.to_string())?;
            let reload_result = loop {
                match updates.recv_timeout(Duration::from_secs(100)) {
                    Ok(Event::OperationFinished {
                        operation: Operation::Reload,
                        automatic: true,
                        result,
                    }) => break result,
                    Ok(Event::Failed(error)) => return Err(error),
                    Ok(_) => {}
                    Err(error) => return Err(error.to_string()),
                }
            };
            reload_result?;
            commands
                .send(ServerCommand::Restart)
                .map_err(|error| error.to_string())?;
            loop {
                match updates.recv_timeout(Duration::from_secs(100)) {
                    Ok(Event::OperationFinished {
                        operation: Operation::Restart,
                        automatic: false,
                        result,
                    }) => return result,
                    Ok(Event::Failed(error)) => return Err(error),
                    Ok(_) => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
        })();
        let _ = commands.send(ServerCommand::Stop);
        drop(commands);
        while let Ok(event) = updates.recv_timeout(Duration::from_secs(5)) {
            if let Event::Failed(error) = event {
                tracing::warn!("Flutter preview stopped with: {error}");
            }
        }
        std::fs::write(source, original).unwrap();
        assert!(result.is_ok(), "automatic reload failed: {result:?}");
    }
}
