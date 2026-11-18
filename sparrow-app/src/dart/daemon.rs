//! Flutter Daemon JSON-RPC client.
//!
//! Spawns `flutter daemon` in the workspace directory and communicates
//! over stdin/stdout using the structured Flutter Daemon protocol.
//!
//! ## Protocol overview
//!
//! - **Requests** (client -> daemon):
//!   `[{"id": 1, "method": "device.enable"}]`
//!   `[{"id": 2, "method": "app.start", "params": {"projectDirectory": "...", "deviceId": "...", "mode": "debug"}}]`
//!   `[{"id": 3, "method": "app.restart", "params": {"appId": "...", "fullRestart": false, "pause": false}}]`
//!   `[{"id": 4, "method": "app.stop", "params": {"appId": "..."}}]`
//!
//! - **Responses** (daemon -> client):
//!   `[{"id": 1, "result": true}]`
//!   `[{"id": 3, "result": {"code": 0, "message": "Reloaded 1 of 645 libraries in 248ms."}}]`
//!
//! - **Events** (daemon -> client):
//!   `[{"event": "daemon.connected", "params": {"version": "0.6.1", "pid": 12345}}]`
//!   `[{"event": "device.added", "params": {"id": "...", "name": "...", "targetPlatform": "..."}}]`
//!   `[{"event": "device.removed", "params": {"id": "..."}}]`
//!   `[{"event": "app.start", "params": {"appId": "...", "deviceId": "...", "directory": "..."}}]`
//!   `[{"event": "app.debugPort", "params": {"appId": "...", "port": 54321, "wsUri": "ws://127.0.0.1:54321/ws"}}]`
//!   `[{"event": "app.started", "params": {"appId": "..."}}]`
//!   `[{"event": "app.progress", "params": {"id": "1", "message": "Building APK...", "finished": false}}]`
//!   `[{"event": "app.log", "params": {"appId": "...", "log": "..."}}]`
//!   `[{"event": "app.stop", "params": {"appId": "..."}}]`

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread,
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::hot_reload::{FlutterDevice, FlutterEmulator};

pub type NotifSender = std::sync::mpsc::Sender<DaemonNotification>;
pub type NotifReceiver = std::sync::mpsc::Receiver<DaemonNotification>;
type ResponseCallback = Box<dyn FnOnce(Result<Value, String>) + Send + 'static>;

/// Events emitted from Flutter daemon to the UI.
#[derive(Debug, Clone)]
pub enum DaemonNotification {
    /// Connected with daemon version and process ID.
    Connected { version: String, pid: u32 },
    /// A new device was attached or discovered.
    DeviceAdded(FlutterDevice),
    /// A device was disconnected.
    DeviceRemoved { id: String },
    /// A device property changed.
    DeviceChanged(FlutterDevice),
    /// Initial or refreshed list of active devices.
    DeviceList(Vec<FlutterDevice>),
    /// List of available cold emulators.
    EmulatorList(Vec<FlutterEmulator>),
    /// An application process started initializing.
    AppStarting { app_id: String, device_id: String },
    /// An application process is fully started and running.
    AppStarted { app_id: String },
    /// Debugger / VM Service WebSocket URI reported by Flutter.
    AppDebugPort { app_id: String, ws_uri: String },
    /// Console output / log line from the running app.
    AppLog { app_id: String, log: String, is_error: bool },
    /// Build or launch progress status.
    AppProgress { message: String, finished: bool },
    /// Hot reload or hot restart outcome.
    AppRestartResult {
        app_id: String,
        success: bool,
        message: String,
        elapsed_ms: Option<u64>,
    },
    /// Application stopped or terminated.
    AppStopped { app_id: String, error: Option<String> },
    /// Non-fatal or protocol error message.
    Error(String),
    /// Daemon process exited.
    Exited,
}

/// Commands sent to the background Flutter daemon worker.
pub enum DaemonCommand {
    EnableDevices,
    GetDevices,
    GetEmulators,
    LaunchEmulator { emulator_id: String },
    StartApp {
        project_dir: PathBuf,
        device_id: Option<String>,
        mode: Option<String>,
        target: Option<String>,
        args: Option<Vec<String>>,
    },
    RestartApp {
        app_id: String,
        full_restart: bool,
        pause: bool,
        reason: Option<String>,
    },
    StopApp {
        app_id: String,
    },
    DetachApp {
        app_id: String,
    },
    Shutdown,
}

/// Persistent Flutter Daemon service handle. Cheap to clone.
#[derive(Clone)]
pub struct FlutterDaemonService {
    tx: Sender<DaemonCommand>,
    running: Arc<AtomicBool>,
    active_app_id: Arc<Mutex<Option<String>>>,
}

pub(crate) struct WorkerShared {
    pub next_id: AtomicI64,
    pub callbacks: Mutex<HashMap<i64, ResponseCallback>>,
    pub active_app_id: Arc<Mutex<Option<String>>>,
}

impl FlutterDaemonService {
    /// Spawn a persistent `flutter daemon` in `workspace_root`.
    /// Returns `(service, notification_receiver)`.
    pub fn new(workspace_root: Option<&Path>) -> (Self, NotifReceiver) {
        let (tx, rx) = unbounded();
        let (notif_tx, notif_rx) = std::sync::mpsc::channel();
        let running = Arc::new(AtomicBool::new(false));
        let active_app_id = Arc::new(Mutex::new(None));

        let shared = Arc::new(WorkerShared {
            next_id: AtomicI64::new(1),
            callbacks: Mutex::new(HashMap::new()),
            active_app_id: active_app_id.clone(),
        });

        let ws_buf = workspace_root.map(|p| p.to_path_buf());
        let shared_for_worker = shared.clone();
        let running_for_worker = running.clone();

        thread::Builder::new()
            .name("FlutterDaemon".into())
            .spawn(move || {
                run_daemon_worker(ws_buf, rx, notif_tx, shared_for_worker, running_for_worker);
            })
            .expect("spawn FlutterDaemon worker thread");

        let service = Self {
            tx: tx.clone(),
            running,
            active_app_id,
        };

        (service, notif_rx)
    }

    /// Whether the underlying `flutter daemon` process is active.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Get the currently active app ID, if any.
    pub fn active_app_id(&self) -> Option<String> {
        self.active_app_id.lock().clone()
    }

    /// Enable device discovery and listening.
    pub fn enable_devices(&self) {
        let _ = self.tx.send(DaemonCommand::EnableDevices);
    }

    /// Request active devices list.
    pub fn get_devices(&self) {
        let _ = self.tx.send(DaemonCommand::GetDevices);
    }

    /// Request available emulators list.
    pub fn get_emulators(&self) {
        let _ = self.tx.send(DaemonCommand::GetEmulators);
    }

    /// Launch a cold emulator by ID.
    pub fn launch_emulator(&self, emulator_id: &str) {
        let _ = self.tx.send(DaemonCommand::LaunchEmulator {
            emulator_id: emulator_id.to_string(),
        });
    }

    /// Start a Flutter application on the specified device.
    pub fn start_app(
        &self,
        project_dir: PathBuf,
        device_id: Option<String>,
        mode: Option<String>,
        target: Option<String>,
        args: Option<Vec<String>>,
    ) {
        let _ = self.tx.send(DaemonCommand::StartApp {
            project_dir,
            device_id,
            mode,
            target,
            args,
        });
    }

    /// Trigger hot reload (`full_restart: false`) or hot restart (`full_restart: true`).
    pub fn restart_app(&self, app_id: &str, full_restart: bool) {
        let _ = self.tx.send(DaemonCommand::RestartApp {
            app_id: app_id.to_string(),
            full_restart,
            pause: false,
            reason: Some("manual".to_string()),
        });
    }

    /// Stop running application.
    pub fn stop_app(&self, app_id: &str) {
        let _ = self.tx.send(DaemonCommand::StopApp {
            app_id: app_id.to_string(),
        });
    }

    /// Detach from running application without killing it.
    pub fn detach_app(&self, app_id: &str) {
        let _ = self.tx.send(DaemonCommand::DetachApp {
            app_id: app_id.to_string(),
        });
    }

    /// Shutdown the daemon.
    pub fn shutdown(&self) {
        let _ = self.tx.send(DaemonCommand::Shutdown);
    }
}

fn run_daemon_worker(
    workspace_root: Option<PathBuf>,
    rx: Receiver<DaemonCommand>,
    notif_tx: NotifSender,
    shared: Arc<WorkerShared>,
    running: Arc<AtomicBool>,
) {
    let flutter_bin = crate::dart::sdk::find_flutter_binary(workspace_root.as_deref())
        .unwrap_or_else(|| PathBuf::from("flutter"));

    let mut cmd = Command::new(&flutter_bin);
    cmd.args(["daemon"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ref root) = workspace_root {
        cmd.current_dir(root);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            let _ = notif_tx.send(DaemonNotification::Error(format!(
                "Failed to spawn flutter daemon ({}): {}",
                flutter_bin.display(),
                err
            )));
            let _ = notif_tx.send(DaemonNotification::Exited);
            return;
        }
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return,
    };
    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => return,
    };
    let stderr = child.stderr.take();

    running.store(true, Ordering::Release);

    // Spawn reader thread for daemon stdout
    let notif_tx_reader = notif_tx.clone();
    let shared_reader = shared.clone();
    let reader_handle = thread::Builder::new()
        .name("FlutterDaemonReader".into())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        let trimmed = line.trim_start_matches('\u{feff}').trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        handle_daemon_line(trimmed, &notif_tx_reader, &shared_reader);
                    }
                    Err(_) => break,
                }
            }
        });

    // Optional stderr logger thread
    if let Some(stderr) = stderr {
        let notif_tx_err = notif_tx.clone();
        thread::Builder::new()
            .name("FlutterDaemonStderr".into())
            .spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            let _ = notif_tx_err.send(DaemonNotification::Error(trimmed.to_string()));
                        }
                    }
                }
            })
            .ok();
    }

    // Writer / Command handling loop
    let mut writer = stdin;

    // Send initial bootstrap commands: device.enable, device.getDevices, emulator.getEmulators
    send_request(&mut writer, &shared, "device.enable", None, None);
    send_request(&mut writer, &shared, "device.getDevices", None, {
        let notif_tx = notif_tx.clone();
        Some(Box::new(move |res| {
            if let Ok(Value::Array(devices_val)) = res {
                let devs: Vec<FlutterDevice> = serde_json::from_value(Value::Array(devices_val)).unwrap_or_default();
                let _ = notif_tx.send(DaemonNotification::DeviceList(devs));
            }
        }))
    });
    send_request(&mut writer, &shared, "emulator.getEmulators", None, {
        let notif_tx = notif_tx.clone();
        Some(Box::new(move |res| {
            if let Ok(Value::Array(emus_val)) = res {
                let emus: Vec<FlutterEmulator> = serde_json::from_value(Value::Array(emus_val)).unwrap_or_default();
                let _ = notif_tx.send(DaemonNotification::EmulatorList(emus));
            }
        }))
    });

    while let Ok(cmd) = rx.recv() {
        match cmd {
            DaemonCommand::EnableDevices => {
                send_request(&mut writer, &shared, "device.enable", None, None);
            }
            DaemonCommand::GetDevices => {
                let notif_tx = notif_tx.clone();
                send_request(&mut writer, &shared, "device.getDevices", None, Some(Box::new(move |res| {
                    if let Ok(Value::Array(devices_val)) = res {
                        let devs: Vec<FlutterDevice> = serde_json::from_value(Value::Array(devices_val)).unwrap_or_default();
                        let _ = notif_tx.send(DaemonNotification::DeviceList(devs));
                    }
                })));
            }
            DaemonCommand::GetEmulators => {
                let notif_tx = notif_tx.clone();
                send_request(&mut writer, &shared, "emulator.getEmulators", None, Some(Box::new(move |res| {
                    if let Ok(Value::Array(emus_val)) = res {
                        let emus: Vec<FlutterEmulator> = serde_json::from_value(Value::Array(emus_val)).unwrap_or_default();
                        let _ = notif_tx.send(DaemonNotification::EmulatorList(emus));
                    }
                })));
            }
            DaemonCommand::LaunchEmulator { emulator_id } => {
                let notif_tx = notif_tx.clone();
                send_request(
                    &mut writer,
                    &shared,
                    "emulator.launch",
                    Some(json!({ "emulatorId": emulator_id })),
                    Some(Box::new(move |res| {
                        if let Err(err) = res {
                            let _ = notif_tx.send(DaemonNotification::Error(format!("Failed to launch emulator: {err}")));
                        }
                    })),
                );
            }
            DaemonCommand::StartApp {
                project_dir,
                device_id,
                mode,
                target,
                args,
            } => {
                let mut params = serde_json::Map::new();
                params.insert("projectDirectory".to_string(), json!(project_dir.to_string_lossy().to_string()));
                if let Some(dev_id) = device_id {
                    params.insert("deviceId".to_string(), json!(dev_id));
                }
                if let Some(m) = mode {
                    params.insert("mode".to_string(), json!(m));
                }
                if let Some(t) = target {
                    params.insert("target".to_string(), json!(t));
                }
                if let Some(a) = args {
                    params.insert("args".to_string(), json!(a));
                }
                let notif_tx = notif_tx.clone();
                send_request(
                    &mut writer,
                    &shared,
                    "app.start",
                    Some(Value::Object(params)),
                    Some(Box::new(move |res| {
                        match res {
                            Ok(val) => {
                                if let Some(started) = val.get("started").and_then(|s| s.as_bool()) {
                                    if !started {
                                        let message = val.get("message").and_then(|m| m.as_str()).unwrap_or("Failed to start app");
                                        let _ = notif_tx.send(DaemonNotification::Error(message.to_string()));
                                    }
                                }
                            }
                            Err(err) => {
                                let _ = notif_tx.send(DaemonNotification::Error(err));
                            }
                        }
                    })),
                );
            }
            DaemonCommand::RestartApp {
                app_id,
                full_restart,
                pause,
                reason,
            } => {
                let params = json!({
                    "appId": app_id,
                    "fullRestart": full_restart,
                    "pause": pause,
                    "reason": reason.unwrap_or_else(|| "manual".to_string()),
                });
                let notif_tx = notif_tx.clone();
                let app_id_clone = app_id.clone();
                send_request(
                    &mut writer,
                    &shared,
                    "app.restart",
                    Some(params),
                    Some(Box::new(move |res| {
                        match res {
                            Ok(val) => {
                                let code = val.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                                let message = val.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
                                let elapsed = extract_elapsed_ms(&message);
                                let _ = notif_tx.send(DaemonNotification::AppRestartResult {
                                    app_id: app_id_clone,
                                    success: code == 0,
                                    message,
                                    elapsed_ms: elapsed,
                                });
                            }
                            Err(err) => {
                                let _ = notif_tx.send(DaemonNotification::AppRestartResult {
                                    app_id: app_id_clone,
                                    success: false,
                                    message: err,
                                    elapsed_ms: None,
                                });
                            }
                        }
                    })),
                );
            }
            DaemonCommand::StopApp { app_id } => {
                let params = json!({ "appId": app_id });
                send_request(&mut writer, &shared, "app.stop", Some(params), None);
            }
            DaemonCommand::DetachApp { app_id } => {
                let params = json!({ "appId": app_id });
                send_request(&mut writer, &shared, "app.detach", Some(params), None);
            }
            DaemonCommand::Shutdown => {
                send_request(&mut writer, &shared, "daemon.shutdown", None, None);
                break;
            }
        }
    }

    running.store(false, Ordering::Release);
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader_handle.map(|h| h.join());
    let _ = notif_tx.send(DaemonNotification::Exited);
}

fn send_request(
    writer: &mut impl Write,
    shared: &WorkerShared,
    method: &str,
    params: Option<Value>,
    callback: Option<ResponseCallback>,
) {
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    if let Some(cb) = callback {
        shared.callbacks.lock().insert(id, cb);
    }

    let mut obj = serde_json::Map::new();
    obj.insert("id".to_string(), json!(id));
    obj.insert("method".to_string(), json!(method));
    if let Some(params) = params {
        obj.insert("params".to_string(), params);
    }

    let payload = json!([Value::Object(obj)]).to_string();
    if let Ok(_) = writer.write_all(payload.as_bytes()) {
        let _ = writer.write_all(b"\n");
        let _ = writer.flush();
    }
}

/// Handle a single raw line emitted by `flutter daemon`.
pub(crate) fn handle_daemon_line(line: &str, notif_tx: &NotifSender, shared: &WorkerShared) {
    let Ok(val) = serde_json::from_str::<Value>(line) else {
        return;
    };

    let items = match val {
        Value::Array(items) => items,
        Value::Object(obj) => vec![Value::Object(obj)],
        _ => return,
    };

    for item in items {
        if let Some(event_name) = item.get("event").and_then(|e| e.as_str()) {
            let params = item.get("params").cloned().unwrap_or(Value::Null);
            handle_daemon_event(event_name, params, notif_tx, shared);
        } else if let Some(id) = item.get("id").and_then(|id| id.as_i64()) {
            let callback = shared.callbacks.lock().remove(&id);
            if let Some(cb) = callback {
                if let Some(error) = item.get("error") {
                    let err_str = match error {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    cb(Err(err_str));
                } else {
                    let result = item.get("result").cloned().unwrap_or(Value::Null);
                    cb(Ok(result));
                }
            }
        }
    }
}

fn handle_daemon_event(
    event: &str,
    params: Value,
    notif_tx: &NotifSender,
    shared: &WorkerShared,
) {
    match event {
        "daemon.connected" => {
            let version = params.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let pid = params.get("pid").and_then(|p| p.as_u64()).unwrap_or(0) as u32;
            let _ = notif_tx.send(DaemonNotification::Connected { version, pid });
        }
        "device.added" => {
            if let Ok(dev) = serde_json::from_value::<FlutterDevice>(params) {
                let _ = notif_tx.send(DaemonNotification::DeviceAdded(dev));
            }
        }
        "device.removed" => {
            if let Some(id) = params.get("id").and_then(|i| i.as_str()) {
                let _ = notif_tx.send(DaemonNotification::DeviceRemoved { id: id.to_string() });
            }
        }
        "device.changed" => {
            if let Ok(dev) = serde_json::from_value::<FlutterDevice>(params) {
                let _ = notif_tx.send(DaemonNotification::DeviceChanged(dev));
            }
        }
        "app.start" => {
            let app_id = params.get("appId").and_then(|a| a.as_str()).unwrap_or("").to_string();
            let device_id = params.get("deviceId").and_then(|d| d.as_str()).unwrap_or("").to_string();
            *shared.active_app_id.lock() = Some(app_id.clone());
            let _ = notif_tx.send(DaemonNotification::AppStarting { app_id, device_id });
        }
        "app.started" => {
            let app_id = params.get("appId").and_then(|a| a.as_str()).unwrap_or("").to_string();
            *shared.active_app_id.lock() = Some(app_id.clone());
            let _ = notif_tx.send(DaemonNotification::AppStarted { app_id });
        }
        "app.debugPort" => {
            let app_id = params.get("appId").and_then(|a| a.as_str()).unwrap_or("").to_string();
            let ws_uri = params.get("wsUri").and_then(|w| w.as_str()).unwrap_or("").to_string();
            let _ = notif_tx.send(DaemonNotification::AppDebugPort { app_id, ws_uri });
        }
        "app.progress" => {
            let message = params.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
            let finished = params.get("finished").and_then(|f| f.as_bool()).unwrap_or(false);
            let _ = notif_tx.send(DaemonNotification::AppProgress { message, finished });
        }
        "app.log" => {
            let app_id = params.get("appId").and_then(|a| a.as_str()).unwrap_or("").to_string();
            let log = params.get("log").and_then(|l| l.as_str()).unwrap_or("").to_string();
            let is_error = params.get("error").and_then(|e| e.as_bool()).unwrap_or(false);
            let _ = notif_tx.send(DaemonNotification::AppLog {
                app_id,
                log,
                is_error,
            });
        }
        "app.stop" => {
            let app_id = params.get("appId").and_then(|a| a.as_str()).unwrap_or("").to_string();
            let error = params.get("error").and_then(|e| e.as_str()).map(|s| s.to_string());
            *shared.active_app_id.lock() = None;
            let _ = notif_tx.send(DaemonNotification::AppStopped { app_id, error });
        }
        "daemon.logMessage" => {
            if let Some(msg) = params.get("message").and_then(|m| m.as_str()) {
                let _ = notif_tx.send(DaemonNotification::Error(msg.to_string()));
            }
        }
        _ => {}
    }
}

/// Helper to parse milliseconds from hot reload messages like
/// "Reloaded 1 of 645 libraries in 248ms."
fn extract_elapsed_ms(message: &str) -> Option<u64> {
    let in_idx = message.rfind(" in ")?;
    let rem = &message[in_idx + 4..];
    let ms_idx = rem.find("ms")?;
    rem[..ms_idx].trim().parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_extract_elapsed_ms() {
        assert_eq!(
            extract_elapsed_ms("Reloaded 1 of 645 libraries in 248ms."),
            Some(248)
        );
        assert_eq!(
            extract_elapsed_ms("Restarted application in 1205ms."),
            Some(1205)
        );
        assert_eq!(extract_elapsed_ms("No time here"), None);
    }

    #[test]
    fn test_daemon_events_parsing() {
        let (notif_tx, notif_rx) = std::sync::mpsc::channel();
        let shared = Arc::new(WorkerShared {
            next_id: AtomicI64::new(1),
            callbacks: Mutex::new(HashMap::new()),
            active_app_id: Arc::new(Mutex::new(None)),
        });

        // 1. daemon.connected
        let json = r#"[{"event":"daemon.connected","params":{"version":"0.6.1","pid":4242}}]"#;
        handle_daemon_line(json, &notif_tx, &shared);
        match notif_rx.recv_timeout(Duration::from_millis(50)).unwrap() {
            DaemonNotification::Connected { version, pid } => {
                assert_eq!(version, "0.6.1");
                assert_eq!(pid, 4242);
            }
            other => panic!("Unexpected notification: {:?}", other),
        }

        // 2. device.added
        let json = r#"[{"event":"device.added","params":{"id":"emulator-5554","name":"Pixel 7 Pro","targetPlatform":"android-arm64","isSupported":true}}]"#;
        handle_daemon_line(json, &notif_tx, &shared);
        match notif_rx.recv_timeout(Duration::from_millis(50)).unwrap() {
            DaemonNotification::DeviceAdded(dev) => {
                assert_eq!(dev.id, "emulator-5554");
                assert_eq!(dev.name, "Pixel 7 Pro");
                assert_eq!(dev.target_platform.as_deref(), Some("android-arm64"));
            }
            other => panic!("Unexpected notification: {:?}", other),
        }

        // 3. app.debugPort
        let json = r#"[{"event":"app.debugPort","params":{"appId":"app_123","port":54321,"wsUri":"ws://127.0.0.1:54321/ws"}}]"#;
        handle_daemon_line(json, &notif_tx, &shared);
        match notif_rx.recv_timeout(Duration::from_millis(50)).unwrap() {
            DaemonNotification::AppDebugPort { app_id, ws_uri } => {
                assert_eq!(app_id, "app_123");
                assert_eq!(ws_uri, "ws://127.0.0.1:54321/ws");
            }
            other => panic!("Unexpected notification: {:?}", other),
        }

        // 4. app.log
        let json = r#"[{"event":"app.log","params":{"appId":"app_123","log":"Hello Flutter!","error":false}}]"#;
        handle_daemon_line(json, &notif_tx, &shared);
        match notif_rx.recv_timeout(Duration::from_millis(50)).unwrap() {
            DaemonNotification::AppLog { app_id, log, is_error } => {
                assert_eq!(app_id, "app_123");
                assert_eq!(log, "Hello Flutter!");
                assert!(!is_error);
            }
            other => panic!("Unexpected notification: {:?}", other),
        }

        // 5. app.progress
        let json = r#"[{"event":"app.progress","params":{"id":"p1","message":"Building APK...","finished":false}}]"#;
        handle_daemon_line(json, &notif_tx, &shared);
        match notif_rx.recv_timeout(Duration::from_millis(50)).unwrap() {
            DaemonNotification::AppProgress { message, finished } => {
                assert_eq!(message, "Building APK...");
                assert!(!finished);
            }
            other => panic!("Unexpected notification: {:?}", other),
        }
    }

    #[test]
    fn test_daemon_response_callback() {
        let (notif_tx, _) = std::sync::mpsc::channel();
        let shared = Arc::new(WorkerShared {
            next_id: AtomicI64::new(1),
            callbacks: Mutex::new(HashMap::new()),
            active_app_id: Arc::new(Mutex::new(None)),
        });

        let (res_tx, res_rx) = std::sync::mpsc::channel();
        shared.callbacks.lock().insert(
            1,
            Box::new(move |res| {
                res_tx.send(res).unwrap();
            }),
        );

        let json = r#"[{"id":1,"result":{"code":0,"message":"Reloaded 1 of 645 libraries in 248ms."}}]"#;
        handle_daemon_line(json, &notif_tx, &shared);

        let result = res_rx.recv_timeout(Duration::from_millis(50)).unwrap().unwrap();
        assert_eq!(result.get("code").and_then(|c| c.as_i64()), Some(0));
    }
}
