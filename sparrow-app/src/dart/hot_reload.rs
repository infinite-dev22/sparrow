//! Flutter `flutter run` integration — hot reload / restart / stop.
//!
//! When `launch` runs, we create a terminal whose process is
//! `flutter run` (wired through the existing `RunDebugProcess`
//! machinery so it shows up in the debug panel and terminal tabs).
//! Subsequent reloads send `r` (or `R` for restart) directly to that
//! terminal's stdin — Flutter's CLI is in raw mode, so single chars
//! are keystrokes.

use std::{rc::Rc, time::Instant};

use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use serde::Deserialize;
use sparrow_rpc::{
    dap_types::{ConfigSource, RunDebugConfig},
    terminal::TermId,
};

use super::daemon::{DaemonNotification, FlutterDaemonService};
use crate::{
    debug::{RunDebugMode, RunDebugProcess},
    panel::kind::PanelKind,
    terminal::panel::TerminalPanelData,
    window_tab::{CommonData, Focus},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterRunState {
    /// No `flutter run` process in this workspace.
    Idle,
    /// `flutter run` is running in the tracked terminal.
    Running,
    /// Previous `flutter run` exited; ready to be relaunched.
    Error,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlutterDevice {
    pub name: String,
    pub id: String,
    #[serde(default)]
    pub is_supported: bool,
    #[serde(default)]
    pub target_platform: Option<String>,
    #[serde(default)]
    pub emulator: Option<bool>,
    #[serde(default)]
    pub sdk: Option<String>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlutterEmulator {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub platform_type: Option<String>,
}

/// Map a device target platform string to the corresponding theme icon constant.
pub fn device_platform_icon(platform: Option<&str>) -> &'static str {
    match platform {
        Some(p) => {
            let lower = p.to_lowercase();
            if lower.contains("android") || lower.contains("ios") || lower.contains("mobile") {
                crate::config::icon::SparrowIcons::DEVICE_MOBILE
            } else if lower.contains("web") || lower.contains("browser") || lower.contains("chrome") {
                crate::config::icon::SparrowIcons::DEVICE_WEB
            } else {
                crate::config::icon::SparrowIcons::DEVICE_DESKTOP
            }
        }
        None => crate::config::icon::SparrowIcons::DEVICE_DESKTOP,
    }
}

#[derive(Clone)]
pub struct FlutterHotReloadData {
    pub state: RwSignal<FlutterRunState>,
    pub active_term: RwSignal<Option<TermId>>,
    pub message: RwSignal<Option<String>>,
    pub devices: RwSignal<Vec<FlutterDevice>>,
    pub selected_device: RwSignal<Option<FlutterDevice>>,
    pub emulators: RwSignal<Vec<FlutterEmulator>>,
    pub is_fetching_devices: RwSignal<bool>,
    pub daemon: RwSignal<Option<FlutterDaemonService>>,
    pub active_app_id: RwSignal<Option<String>>,
    pub progress_message: RwSignal<Option<String>>,
    pub ws_uri: RwSignal<Option<String>>,
    pub run_logs: RwSignal<im::Vector<String>>,
    pub common: Rc<CommonData>,
    pub terminal: TerminalPanelData,
}

impl FlutterHotReloadData {
    pub fn new(
        cx: Scope,
        common: Rc<CommonData>,
        terminal: TerminalPanelData,
    ) -> Self {
        let this = Self {
            state: cx.create_rw_signal(FlutterRunState::Idle),
            active_term: cx.create_rw_signal(None),
            message: cx.create_rw_signal(None),
            devices: cx.create_rw_signal(Vec::new()),
            selected_device: cx.create_rw_signal(None),
            emulators: cx.create_rw_signal(Vec::new()),
            is_fetching_devices: cx.create_rw_signal(false),
            daemon: cx.create_rw_signal(None),
            active_app_id: cx.create_rw_signal(None),
            progress_message: cx.create_rw_signal(None),
            ws_uri: cx.create_rw_signal(None),
            run_logs: cx.create_rw_signal(im::Vector::new()),
            common,
            terminal,
        };

        if this.is_flutter_project() {
            this.init_daemon();
        }

        this
    }

    /// Initialize the persistent Flutter daemon service.
    pub fn init_daemon(&self) {
        if !self.is_flutter_project() {
            return;
        }
        if self.daemon.get_untracked().is_some() {
            return;
        }

        let (daemon_svc, daemon_rx) = FlutterDaemonService::new(self.common.workspace.path.as_deref());
        self.daemon.set(Some(daemon_svc));

        let notif_signal = floem::ext_event::create_signal_from_channel(daemon_rx);
        let this = self.clone();
        self.common.scope.create_effect(move |_| {
            if let Some(notif) = notif_signal.get() {
                this.apply_daemon_notification(notif);
            }
        });
    }

    /// Process a notification event from the Flutter daemon client.
    pub fn apply_daemon_notification(&self, notif: DaemonNotification) {
        match notif {
            DaemonNotification::Connected { version, pid } => {
                tracing::info!("Flutter daemon connected: v{}, pid={}", version, pid);
            }
            DaemonNotification::DeviceAdded(dev) => {
                self.devices.update(|devs| {
                    if let Some(pos) = devs.iter().position(|d| d.id == dev.id) {
                        devs[pos] = dev.clone();
                    } else {
                        devs.push(dev.clone());
                    }
                });
                if self.selected_device.get_untracked().is_none() {
                    self.selected_device.set(Some(dev));
                }
            }
            DaemonNotification::DeviceRemoved { id } => {
                self.devices.update(|devs| {
                    devs.retain(|d| d.id != id);
                });
                if let Some(sel) = self.selected_device.get_untracked() {
                    if sel.id == id {
                        let next = self.devices.with_untracked(|d| d.first().cloned());
                        self.selected_device.set(next);
                    }
                }
            }
            DaemonNotification::DeviceChanged(dev) => {
                self.devices.update(|devs| {
                    if let Some(pos) = devs.iter().position(|d| d.id == dev.id) {
                        devs[pos] = dev.clone();
                    }
                });
                if let Some(sel) = self.selected_device.get_untracked() {
                    if sel.id == dev.id {
                        self.selected_device.set(Some(dev));
                    }
                }
            }
            DaemonNotification::DeviceList(devs) => {
                if !devs.is_empty() {
                    match self.selected_device.get_untracked() {
                        None => self.selected_device.set(Some(devs[0].clone())),
                        Some(current) => {
                            let next = devs
                                .iter()
                                .find(|d| d.id == current.id)
                                .unwrap_or(&devs[0])
                                .clone();
                            self.selected_device.set(Some(next));
                        }
                    }
                } else {
                    self.selected_device.set(None);
                }
                self.devices.set(devs);
                self.is_fetching_devices.set(false);
            }
            DaemonNotification::EmulatorList(emus) => {
                self.emulators.set(emus);
            }
            DaemonNotification::AppStarting { app_id, .. } => {
                self.active_app_id.set(Some(app_id));
                self.state.set(FlutterRunState::Running);
                self.add_log("⚡ Launching Flutter application...");
            }
            DaemonNotification::AppStarted { app_id } => {
                self.active_app_id.set(Some(app_id));
                self.state.set(FlutterRunState::Running);
                self.progress_message.set(None);
                self.add_log("✔ Flutter application started.");
            }
            DaemonNotification::AppDebugPort { ws_uri, .. } => {
                self.ws_uri.set(Some(ws_uri.clone()));
                self.add_log(format!("🔗 Dart VM service listening on {ws_uri}"));
            }
            DaemonNotification::AppProgress { message, finished } => {
                if !finished {
                    self.progress_message.set(Some(message.clone()));
                    self.add_log(format!("⏳ {message}"));
                } else {
                    self.progress_message.set(None);
                }
            }
            DaemonNotification::AppRestartResult { message, .. } => {
                self.message.set(Some(message.clone()));
                self.add_log(format!("⚡ {message}"));
            }
            DaemonNotification::AppStopped { app_id: _, error } => {
                self.active_app_id.set(None);
                self.state.set(FlutterRunState::Idle);
                self.progress_message.set(None);
                self.ws_uri.set(None);
                if let Some(err) = error {
                    self.message.set(Some(err.clone()));
                    self.add_log(format!("❌ Application stopped with error: {err}"));
                } else {
                    self.add_log("⏹ Application stopped.");
                }
            }
            DaemonNotification::AppLog { log, is_error, .. } => {
                let prefix = if is_error { "⚠ " } else { "" };
                self.add_log(format!("{prefix}{log}"));
            }
            DaemonNotification::Error(err) => {
                self.message.set(Some(err.clone()));
                self.add_log(format!("❌ Error: {err}"));
            }
            DaemonNotification::Exited => {
                self.daemon.set(None);
                self.add_log("⏹ Flutter daemon exited.");
            }
        }
    }

    pub fn add_log(&self, line: impl Into<String>) {
        self.run_logs.update(|l| l.push_back(line.into()));
    }

    pub fn clear_logs(&self) {
        self.run_logs.set(im::Vector::new());
    }

    /// True if the workspace root contains a `pubspec.yaml` (i.e. this is
    /// a Flutter/Dart project worth showing the button for).
    pub fn is_flutter_project(&self) -> bool {
        self.common
            .workspace
            .path
            .as_ref()
            .map(|p| p.join("pubspec.yaml").is_file())
            .unwrap_or(false)
    }

    /// Asynchronously fetch available Flutter devices and emulators.
    pub fn fetch_devices(&self) {
        if !self.is_flutter_project() {
            return;
        }

        if let Some(daemon) = self.daemon.get_untracked() {
            if daemon.is_running() {
                daemon.get_devices();
                daemon.get_emulators();
                return;
            }
        }

        if self.is_fetching_devices.get_untracked() {
            return;
        }
        self.is_fetching_devices.set(true);

        let scope = self.common.scope;
        let devices_signal = self.devices;
        let selected_signal = self.selected_device;
        let emulators_signal = self.emulators;
        let fetching_signal = self.is_fetching_devices;

        // Runs on the UI thread: Scope and signals never cross threads.
        let send = floem::ext_event::create_ext_action(
            scope,
            move |(devs, emus): (Vec<FlutterDevice>, Vec<FlutterEmulator>)| {
                fetching_signal.set(false);
                if !devs.is_empty() {
                    match selected_signal.get_untracked() {
                        None => selected_signal.set(Some(devs[0].clone())),
                        Some(current) => {
                            // Preserve selection if it still exists
                            let next = devs
                                .iter()
                                .find(|d| d.id == current.id)
                                .unwrap_or(&devs[0])
                                .clone();
                            selected_signal.set(Some(next));
                        }
                    }
                } else {
                    selected_signal.set(None);
                }
                devices_signal.set(devs);
                emulators_signal.set(emus);
            },
        );

        let ws_path = self.common.workspace.path.clone();
        std::thread::spawn(move || {
            let flutter_bin = crate::dart::sdk::find_flutter_binary(ws_path.as_deref())
                .unwrap_or_else(|| std::path::PathBuf::from("flutter"));

            let devices = std::process::Command::new(&flutter_bin)
                .args(["devices", "--machine"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    // Strip UTF-8 BOM if present
                    let json_str = stdout.trim_start_matches('\u{feff}');
                    serde_json::from_str::<Vec<FlutterDevice>>(json_str)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|d| d.is_supported)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let emulators = std::process::Command::new(&flutter_bin)
                .args(["emulators", "--machine"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let json_str = stdout.trim_start_matches('\u{feff}');
                    serde_json::from_str::<Vec<FlutterEmulator>>(json_str)
                        .unwrap_or_default()
                })
                .unwrap_or_default();

            send((devices, emulators));
        });
    }

    /// Launch a cold emulator by ID using `flutter emulators --launch <id>`.
    pub fn launch_emulator(&self, emulator_id: &str) {
        if let Some(daemon) = self.daemon.get_untracked() {
            if daemon.is_running() {
                daemon.launch_emulator(emulator_id);
                return;
            }
        }

        let id = emulator_id.to_string();
        let ws_path = self.common.workspace.path.clone();
        let scope = self.common.scope;
        let this = self.clone();

        let trigger_refresh = floem::ext_event::create_ext_action(scope, move |()| {
            this.fetch_devices();
        });

        std::thread::spawn(move || {
            let flutter_bin = crate::dart::sdk::find_flutter_binary(ws_path.as_deref())
                .unwrap_or_else(|| std::path::PathBuf::from("flutter"));
            let _ = std::process::Command::new(&flutter_bin)
                .args(["emulators", "--launch", &id])
                .output();

            // Give the emulator a short moment to start, then refresh devices
            std::thread::sleep(std::time::Duration::from_millis(1500));
            trigger_refresh(());
        });
    }

    /// Start `flutter run` in a new terminal (or reuse the existing
    /// stopped one, so relaunch is instant).
    pub fn launch(&self) {
        if self.state.get_untracked() == FlutterRunState::Running {
            return;
        }

        let cwd = self
            .common
            .workspace
            .path
            .as_ref()
            .and_then(|p| p.to_str().map(|s| s.to_string()));

        // Fetch device ID if one is selected
        let device_id = self.selected_device.get_untracked().map(|d| d.id);

        // If daemon is active and we have a workspace path, also dispatch start_app via daemon
        if let Some(daemon) = self.daemon.get_untracked() {
            if daemon.is_running() {
                if let Some(ref path) = self.common.workspace.path {
                    daemon.start_app(
                        path.clone(),
                        device_id.clone(),
                        Some("debug".to_string()),
                        None,
                        None,
                    );
                }
            }
        }

        // Reuse the existing terminal if it's still alive but stopped.
        if let Some(term_id) = self.active_term.get_untracked() {
            if let Some(terminal) = self.terminal.get_terminal(&term_id) {
                let stopped = terminal.run_debug.with_untracked(|r| {
                    r.as_ref().map(|p| p.stopped).unwrap_or(true)
                });
                if stopped {
                    // Restart in place — same terminal tab, fresh process.
                    terminal.new_process(Some(RunDebugProcess {
                        mode: RunDebugMode::Run,
                        config: flutter_config(cwd, device_id),
                        stopped: false,
                        created: Instant::now(),
                        is_prelaunch: false,
                    }));
                    self.state.set(FlutterRunState::Running);
                    self.message.set(None);
                    self.common
                        .focus
                        .set(Focus::Panel(PanelKind::Terminal));
                    return;
                }
            }
        }

        // Fresh launch — new terminal tab.
        let process = RunDebugProcess {
            mode: RunDebugMode::Run,
            config: flutter_config(cwd, device_id),
            stopped: false,
            created: Instant::now(),
            is_prelaunch: false,
        };
        let tab = self.terminal.new_tab_run_debug(Some(process), None);
        match tab.active_terminal(false).map(|t| t.term_id) {
            Some(term_id) => {
                self.active_term.set(Some(term_id));
                self.state.set(FlutterRunState::Running);
                self.message.set(None);
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }
            None => {
                self.state.set(FlutterRunState::Error);
                self.message
                    .set(Some("Failed to create terminal for flutter".to_string()));
            }
        }
    }

    /// Send hot reload command to daemon or active terminal.
    pub fn hot_reload(&self) {
        if let Some(daemon) = self.daemon.get_untracked() {
            if let Some(app_id) = self.active_app_id.get_untracked() {
                daemon.restart_app(&app_id, false);
            }
        }

        if let Some(term_id) = self.active_term.get_untracked() {
            self.common.proxy.terminal_write(term_id, "r".to_string());
        } else if self.active_app_id.get_untracked().is_none() {
            self.launch();
        }
    }

    /// Send hot restart command to daemon or active terminal.
    pub fn hot_restart(&self) {
        if let Some(daemon) = self.daemon.get_untracked() {
            if let Some(app_id) = self.active_app_id.get_untracked() {
                daemon.restart_app(&app_id, true);
            }
        }

        if let Some(term_id) = self.active_term.get_untracked() {
            self.common.proxy.terminal_write(term_id, "R".to_string());
        } else if self.active_app_id.get_untracked().is_none() {
            self.launch();
        }
    }

    /// Re-initialize the daemon and refresh devices.
    pub fn restart_daemon(&self) {
        if let Some(daemon) = self.daemon.get_untracked() {
            daemon.shutdown();
        }
        self.daemon.set(None);
        self.devices.set(Vec::new());
        self.selected_device.set(None);
        self.init_daemon();
        self.fetch_devices();
    }

    /// Send stop command to daemon or active terminal.
    pub fn stop(&self) {
        if let Some(daemon) = self.daemon.get_untracked() {
            if let Some(app_id) = self.active_app_id.get_untracked() {
                daemon.stop_app(&app_id);
            }
        }

        if let Some(term_id) = self.active_term.get_untracked() {
            self.common.proxy.terminal_write(term_id, "q".to_string());
        }
    }
}

fn flutter_config(cwd: Option<String>, device_id: Option<String>) -> RunDebugConfig {
    let mut args = vec!["run".to_string()];
    if let Some(id) = device_id {
        args.push("-d".to_string());
        args.push(id);
    }

    let program = crate::dart::sdk::find_flutter_binary(cwd.as_ref().map(std::path::Path::new))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "flutter".to_string());

    RunDebugConfig {
        ty: None,
        name: "Flutter Run".to_string(),
        program,
        args: Some(args),
        cwd,
        env: None,
        prelaunch: None,
        flutter_options: Default::default(),
        debug_adapter: None,
        debug_command: None,
        dap_id: Default::default(),
        tracing_output: false,
        config_source: ConfigSource::Palette,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_platform_icon_mapping() {
        assert_eq!(
            device_platform_icon(Some("android-arm64")),
            crate::config::icon::SparrowIcons::DEVICE_MOBILE
        );
        assert_eq!(
            device_platform_icon(Some("ios")),
            crate::config::icon::SparrowIcons::DEVICE_MOBILE
        );
        assert_eq!(
            device_platform_icon(Some("web-javascript")),
            crate::config::icon::SparrowIcons::DEVICE_WEB
        );
        assert_eq!(
            device_platform_icon(Some("chrome")),
            crate::config::icon::SparrowIcons::DEVICE_WEB
        );
        assert_eq!(
            device_platform_icon(Some("darwin-arm64")),
            crate::config::icon::SparrowIcons::DEVICE_DESKTOP
        );
        assert_eq!(
            device_platform_icon(Some("linux-x64")),
            crate::config::icon::SparrowIcons::DEVICE_DESKTOP
        );
        assert_eq!(
            device_platform_icon(None),
            crate::config::icon::SparrowIcons::DEVICE_DESKTOP
        );
    }

    #[test]
    fn test_flutter_device_deserialize() {
        let json = r#"[
            {
                "name": "Pixel 7 Pro",
                "id": "emulator-5554",
                "isSupported": true,
                "targetPlatform": "android-arm64",
                "emulator": true,
                "sdk": "Android 14.0"
            }
        ]"#;
        let devices: Vec<FlutterDevice> = serde_json::from_str(json).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Pixel 7 Pro");
        assert_eq!(devices[0].id, "emulator-5554");
        assert!(devices[0].is_supported);
        assert_eq!(devices[0].target_platform.as_deref(), Some("android-arm64"));
    }

    #[test]
    fn test_flutter_emulator_deserialize() {
        let json = r#"[
            {
                "id": "pixel_7",
                "name": "Pixel 7 Pro API 34",
                "category": "mobile",
                "platformType": "android"
            }
        ]"#;
        let emus: Vec<FlutterEmulator> = serde_json::from_str(json).unwrap();
        assert_eq!(emus.len(), 1);
        assert_eq!(emus[0].id, "pixel_7");
        assert_eq!(emus[0].name, "Pixel 7 Pro API 34");
        assert_eq!(emus[0].platform_type.as_deref(), Some("android"));
    }
}