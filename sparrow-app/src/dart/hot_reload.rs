//! Flutter `flutter run` integration — hot reload / restart / stop.
//!
//! Selected run configurations create one tracked terminal-backed CLI session.
//! Reload and restart write Flutter's interactive commands to that session;
//! device discovery uses a separate daemon and does not launch applications.

use std::rc::Rc;

use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use serde::Deserialize;
use sparrow_rpc::{
    terminal::TermId,
};

use super::daemon::{DaemonNotification, FlutterDaemonService};
use crate::{
    terminal::panel::TerminalPanelData,
    window_tab::CommonData,
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
    #[serde(default, alias = "platform")]
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
        if let Some(terminal) = self.active_term.get_untracked().and_then(|id| self.terminal.get_terminal(&id)) {
            use alacritty_terminal::vte::ansi::{Handler, ClearMode};
            let raw = terminal.raw.get_untracked();
            let mut raw = raw.write();
            raw.term.clear_screen(ClearMode::All);
            raw.term.clear_screen(ClearMode::Saved);
            self.common.view_id.get_untracked().request_paint();
        }
    }

    /// True when the manifest identifies a Flutter project. Plain Dart
    /// packages do not start device discovery or show Flutter-only actions.
    pub fn is_flutter_project(&self) -> bool {
        self.common
            .workspace
            .path
            .as_ref()
            .map(|p| super::is_flutter_project(p))
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

    /// Reload/restart never launches another process and only addresses the
    /// currently owned CLI session. Device discovery remains daemon-owned.
    fn reload_debug_session(&self, restart: bool) -> bool {
        let session = self.terminal.run_debug_process(false).into_iter().rev().find(|(_, p)| {
            !p.stopped && p.mode == crate::debug::RunDebugMode::Debug
                && super::run_config::is_flutter_target(&p.config)
        });
        if let Some((_, process)) = session {
            self.common.proxy.dap_hot_reload(process.config.dap_id, restart);
            true
        } else { false }
    }

    pub fn hot_reload(&self) {
        if self.reload_debug_session(false) { return; }
        if self.state.get_untracked() != FlutterRunState::Running { return; }
        if let Some(id) = self.active_term.get_untracked() {
            self.common.proxy.terminal_write(id, "r".into());
        }
    }
    pub fn hot_restart(&self) {
        if self.reload_debug_session(true) { return; }
        if self.state.get_untracked() != FlutterRunState::Running { return; }
        if let Some(id) = self.active_term.get_untracked() {
            self.common.proxy.terminal_write(id, "R".into());
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

    pub fn stop(&self) {
        if self.state.get_untracked() != FlutterRunState::Running { return; }
        if let Some(id) = self.active_term.get_untracked() {
            // Closing the owned PTY also handles build failures/hung startups,
            // where Flutter is not yet accepting its interactive q command.
            self.terminal.stop_run_debug(id);
        }
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
    fn daemon_platform_field_is_preserved() {
        for platform in ["linux-x64", "web-javascript"] {
            let device: FlutterDevice = serde_json::from_value(serde_json::json!({
                "id": "target", "name": "Target", "platform": platform
            })).unwrap();
            assert_eq!(device.target_platform.as_deref(), Some(platform));
        }
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