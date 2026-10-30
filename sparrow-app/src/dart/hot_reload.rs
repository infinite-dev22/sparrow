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
use sparrow_rpc::{
    dap_types::{ConfigSource, RunDebugConfig},
    terminal::TermId,
};

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

#[derive(Clone)]
pub struct FlutterHotReloadData {
    pub state: RwSignal<FlutterRunState>,
    pub active_term: RwSignal<Option<TermId>>,
    pub message: RwSignal<Option<String>>,
    pub common: Rc<CommonData>,
    pub terminal: TerminalPanelData,
}

impl FlutterHotReloadData {
    pub fn new(
        cx: Scope,
        common: Rc<CommonData>,
        terminal: TerminalPanelData,
    ) -> Self {
        Self {
            state: cx.create_rw_signal(FlutterRunState::Idle),
            active_term: cx.create_rw_signal(None),
            message: cx.create_rw_signal(None),
            common,
            terminal,
        }
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
                        config: flutter_config(cwd),
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
            config: flutter_config(cwd),
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

    /// Send `r` to the running `flutter run` — hot reload.
    pub fn hot_reload(&self) {
        let Some(term_id) = self.active_term.get_untracked() else {
            return self.launch();
        };
        self.common.proxy.terminal_write(term_id, "r".to_string());
    }

    /// Send `R` to the running `flutter run` — hot restart.
    pub fn hot_restart(&self) {
        let Some(term_id) = self.active_term.get_untracked() else {
            return self.launch();
        };
        self.common.proxy.terminal_write(term_id, "R".to_string());
    }

    /// Send `q` to the running `flutter run` — graceful shutdown.
    /// The terminal-exit effect will transition state back to Idle
    /// shortly after.
    pub fn stop(&self) {
        let Some(term_id) = self.active_term.get_untracked() else {
            return;
        };
        self.common.proxy.terminal_write(term_id, "q".to_string());
    }
}

fn flutter_config(cwd: Option<String>) -> RunDebugConfig {
    RunDebugConfig {
        ty: None,
        name: "Flutter Run".to_string(),
        program: "flutter".to_string(),
        args: Some(vec!["run".to_string()]),
        cwd,
        env: None,
        prelaunch: None,
        debug_command: None,
        dap_id: Default::default(),
        tracing_output: false,
        config_source: ConfigSource::Palette,
    }
}