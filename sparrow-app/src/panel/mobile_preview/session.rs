//! Reactive state and lifetime of a simulated device.
use std::{cell::RefCell, path::PathBuf, rc::Rc};

use crossbeam_channel::Sender;
use floem::{
    ext_event::create_signal_from_channel,
    peniko::Image,
    reactive::{RwSignal, SignalGet, SignalUpdate, create_effect, create_rw_signal},
};

use super::{
    device::{DeviceChoice, Display, PreviewLocale, Viewport},
    renderer::{self, Input, Update},
    server::{self, Mode},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Screen {
    Home,
    App,
    Recents,
    Files,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct App {
    pub url: String,
    pub widgets: bool,
}

#[derive(Clone)]
pub(super) struct Session {
    pub device: RwSignal<DeviceChoice>,
    pub locale: RwSignal<PreviewLocale>,
    pub landscape: RwSignal<bool>,
    pub folded: RwSignal<bool>,
    pub dark: RwSignal<bool>,
    pub gestures: RwSignal<bool>,
    pub status_sticky: RwSignal<bool>,
    pub navigation_sticky: RwSignal<bool>,
    pub custom_width: RwSignal<String>,
    pub custom_height: RwSignal<String>,
    pub custom_scale: RwSignal<String>,
    pub screen: RwSignal<Screen>,
    pub active: RwSignal<Option<App>>,
    pub recents: RwSignal<Vec<App>>,
    pub frame: RwSignal<Option<Image>>,
    pub status: RwSignal<String>,
    pub can_reload: RwSignal<bool>,
    pub auto_reload_on_save: RwSignal<bool>,
    pub flutter_runtime_version: RwSignal<String>,
    pub logs: RwSignal<Vec<String>>,
    pub bars: RwSignal<bool>,
    pub panel_visible: RwSignal<bool>,
    bar_sequence: RwSignal<u64>,
    generation: RwSignal<u64>,
    server_generations: [RwSignal<u64>; 2],
    server_urls: [RwSignal<Option<String>>; 2],
    engine: Rc<RefCell<Option<Sender<Input>>>>,
    servers: [Rc<RefCell<Option<Sender<server::ServerCommand>>>>; 2],
    pub files: Rc<RefCell<Option<tempfile::TempDir>>>,
    pub files_revision: RwSignal<u64>,
}

impl Session {
    pub fn new(auto_reload_on_save: bool) -> Self {
        let session = Self {
            device: create_rw_signal(DeviceChoice::Preset("pixel-10".into())),
            locale: create_rw_signal(PreviewLocale::EnglishUnitedStates),
            landscape: create_rw_signal(false),
            folded: create_rw_signal(false),
            dark: create_rw_signal(false),
            gestures: create_rw_signal(true),
            status_sticky: create_rw_signal(true),
            navigation_sticky: create_rw_signal(true),
            custom_width: create_rw_signal("400".into()),
            custom_height: create_rw_signal("800".into()),
            custom_scale: create_rw_signal("2".into()),
            screen: create_rw_signal(Screen::Home),
            active: create_rw_signal(None),
            recents: create_rw_signal(Vec::new()),
            frame: create_rw_signal(None),
            status: create_rw_signal(String::new()),
            can_reload: create_rw_signal(false),
            auto_reload_on_save: create_rw_signal(auto_reload_on_save),
            flutter_runtime_version: create_rw_signal(
                "Flutter version unavailable".into(),
            ),
            logs: create_rw_signal(Vec::new()),
            bars: create_rw_signal(true),
            panel_visible: create_rw_signal(true),
            bar_sequence: create_rw_signal(0),
            generation: create_rw_signal(0),
            server_generations: std::array::from_fn(|_| create_rw_signal(0)),
            server_urls: std::array::from_fn(|_| create_rw_signal(None)),
            engine: Default::default(),
            servers: Default::default(),
            files: Default::default(),
            files_revision: create_rw_signal(0),
        };
        let update = session.clone();
        create_effect(move |_| {
            if let Some(size) = update.viewport() {
                update.send(Input::Configure(size));
            }
        });
        let visible = session.clone();
        create_effect(move |_| {
            let shown = visible.panel_visible.get();
            visible
                .send(Input::Visible(visible.screen.get() == Screen::App && shown));
        });
        let bars = session.clone();
        create_effect(move |_| {
            bars.screen.get();
            bars.status_sticky.get();
            bars.navigation_sticky.get();
            bars.reveal_bars();
        });
        session
    }

    fn record_log(&self, message: &'static str) {
        self.logs.update(|logs| {
            logs.push(message.into());
            if logs.len() > 50 {
                logs.drain(..logs.len() - 50);
            }
        });
    }

    pub fn diagnostic_report(&self) -> String {
        let device = self.device.get_untracked();
        let device_name = device
            .device()
            .map_or("Flexible device", |preset| preset.name.as_str());
        let viewport = self
            .display()
            .map(|display| {
                format!(
                    "{}x{} pixels at {:.2}x",
                    display.pixels.0, display.pixels.1, display.scale
                )
            })
            .unwrap_or_else(|| "Invalid flexible viewport".into());
        let mode = self
            .active
            .get_untracked()
            .map_or("No active preview", |app| {
                if app.widgets {
                    "Flutter widget preview"
                } else {
                    "Flutter web app"
                }
            });
        let logs = self.logs.get_untracked().join("\n");
        format!(
            "Sparrow {} Flutter Preview\nOS: {}\nRuntime: {}\nMode: {mode}\nDevice: {device_name}\nViewport: {viewport}\nLocale: {}\nDark mode: {}\nAuto reload on save: {}\nRecent events:\n{logs}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            self.flutter_runtime_version.get_untracked(),
            self.locale.get_untracked().code(),
            self.dark.get_untracked(),
            self.auto_reload_on_save.get_untracked(),
        )
    }

    pub fn display(&self) -> Option<Display> {
        if let Some(device) = self.device.get().device() {
            Some(if self.folded.get() {
                device.cover.unwrap_or(device.display)
            } else {
                device.display
            })
        } else {
            Viewport::custom(
                &self.custom_width.get(),
                &self.custom_height.get(),
                &self.custom_scale.get(),
            )
        }
    }

    pub fn viewport(&self) -> Option<Viewport> {
        let dark = self.dark.get();
        Some(Viewport::new(
            self.display()?,
            self.landscape.get(),
            dark,
            self.device.get().device().is_some_and(|device| device.ios),
            self.locale.get(),
        ))
    }

    pub fn send(&self, input: Input) {
        if let Some(engine) = self.engine.borrow().as_ref() {
            let _ = engine.send(input);
        }
    }

    pub fn reveal_bars(&self) {
        self.bars.set(true);
        let bars = self.bars;
        let sequence = self.bar_sequence;
        let next = sequence.get_untracked() + 1;
        sequence.set(next);
        floem::action::exec_after(std::time::Duration::from_secs(3), move |_| {
            if sequence.try_get_untracked() == Some(next) {
                bars.set(false);
            }
        });
    }

    pub fn open(&self, entry: App) {
        if !url::Url::parse(&entry.url)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
        {
            self.status
                .set("Enter an HTTP or HTTPS preview URL.".into());
            return;
        }
        let previous = self.active.get_untracked();
        self.active.set(Some(entry.clone()));
        let Some(viewport) = self.viewport() else {
            self.active.set(previous);
            self.status
                .set("Enter valid flexible-device dimensions and scale.".into());
            return;
        };
        self.recents.update(|apps| {
            apps.retain(|app| app != &entry);
            apps.insert(0, entry.clone());
            apps.truncate(12);
        });
        self.screen.set(Screen::App);
        if self.engine.borrow().is_some() {
            if previous.as_ref() != Some(&entry) {
                self.frame.set(None);
                self.status.set("Loading preview…".into());
                self.send(Input::Open(entry.url));
            }
            return;
        }

        self.status.set("Starting preview renderer…".into());
        let generation = self.generation.get_untracked() + 1;
        self.generation.set(generation);
        let (input, output) = renderer::start(entry.url, viewport);
        self.engine.replace(Some(input));
        self.send(Input::Visible(self.panel_visible.get_untracked()));
        let updates = create_signal_from_channel(output);
        let session = self.clone();
        create_effect(move |_| {
            if session.generation.get_untracked() != generation {
                return;
            }
            match updates.get() {
                Some(Update::Frame(frame)) => {
                    if session.frame.get_untracked().is_none() {
                        session.status.set(String::new());
                    }
                    session.frame.set(Some(frame));
                }
                Some(Update::Home) => session.screen.set(Screen::Home),
                Some(Update::Failed(error)) => {
                    session.engine.borrow_mut().take();
                    session.status.set(format!("Preview unavailable. {error}"));
                }
                None => {}
            }
        });
    }

    pub fn start_server(
        &self,
        flutter: PathBuf,
        root: PathBuf,
        mode: Mode,
        auto_reload_on_save: bool,
    ) {
        let slot = usize::from(mode == Mode::Widgets);
        let server = self.servers[slot].clone();
        let generation_signal = self.server_generations[slot];
        let server_url = self.server_urls[slot];
        if let Some(previous) = server.borrow_mut().take() {
            let _ = previous.send(server::ServerCommand::Stop);
        }
        self.can_reload.set(false);
        self.auto_reload_on_save.set(auto_reload_on_save);
        self.record_log(match mode {
            Mode::App => "Starting Flutter web server.",
            Mode::Widgets => "Starting Flutter Widget Previewer.",
        });
        if let Some(previous) = server_url.get_untracked() {
            self.recents
                .update(|apps| apps.retain(|app| app.url != previous));
            server_url.set(None);
        }
        let generation = generation_signal.get_untracked() + 1;
        generation_signal.set(generation);
        self.status.set(
            match mode {
                Mode::App => "Starting Flutter web server…",
                Mode::Widgets => "Starting Flutter Widget Previewer…",
            }
            .into(),
        );
        let (commands, updates) =
            server::start(flutter, root, mode, auto_reload_on_save);
        server.replace(Some(commands));
        let updates = create_signal_from_channel(updates);
        let session = self.clone();
        create_effect(move |_| {
            if generation_signal.get_untracked() != generation {
                return;
            }
            match updates.get() {
                Some(server::Event::Ready { url, app_id }) => {
                    session
                        .can_reload
                        .set(mode == Mode::App && app_id.is_some());
                    server_url.set(Some(url.clone()));
                    session.record_log(match mode {
                        Mode::App => "Flutter app preview is ready.",
                        Mode::Widgets => "Flutter Widget Previewer is ready.",
                    });
                    session.status.set(match mode {
                        Mode::App => "Flutter app preview is ready.".into(),
                        Mode::Widgets => {
                            "Widget Previewer is ready. Select an @Preview to inspect it."
                                .into()
                        }
                    });
                    session.open(App {
                        url,
                        widgets: mode == Mode::Widgets,
                    });
                }
                Some(server::Event::Failed(error)) => {
                    server.borrow_mut().take();
                    session.can_reload.set(false);
                    session.record_log(
                        "Flutter preview process failed; status has details.",
                    );
                    session.status.set(error);
                }
                Some(server::Event::Notice(message)) => {
                    session.record_log("Flutter preview reported a watcher notice.");
                    session.status.set(message);
                }
                Some(server::Event::RuntimeVersion(version)) => {
                    session.flutter_runtime_version.set(version);
                }
                Some(server::Event::OperationStarted {
                    operation,
                    automatic,
                }) => {
                    let name = if automatic {
                        "Auto reloading"
                    } else {
                        match operation {
                            server::Operation::Reload => "Hot reloading",
                            server::Operation::Restart => "Hot restarting",
                        }
                    };
                    session.record_log(match (operation, automatic) {
                        (server::Operation::Reload, true) => {
                            "Automatic reload started."
                        }
                        (server::Operation::Reload, false) => "Hot reload started.",
                        (server::Operation::Restart, _) => "Hot restart started.",
                    });
                    session.status.set(format!("{name}…"));
                }
                Some(server::Event::OperationFinished {
                    operation,
                    automatic,
                    result,
                }) => {
                    let name = match (operation, automatic) {
                        (server::Operation::Reload, true) => "Auto reload",
                        (server::Operation::Reload, false) => "Hot reload",
                        (server::Operation::Restart, _) => "Hot restart",
                    };
                    session.record_log(
                        match (operation, automatic, result.is_ok()) {
                            (server::Operation::Reload, true, true) => {
                                "Automatic reload completed."
                            }
                            (server::Operation::Reload, true, false) => {
                                "Automatic reload failed."
                            }
                            (server::Operation::Reload, false, true) => {
                                "Hot reload completed."
                            }
                            (server::Operation::Reload, false, false) => {
                                "Hot reload failed."
                            }
                            (server::Operation::Restart, _, true) => {
                                "Hot restart completed."
                            }
                            (server::Operation::Restart, _, false) => {
                                "Hot restart failed."
                            }
                        },
                    );
                    session.status.set(match result {
                        Ok(()) => format!("{name} complete."),
                        Err(error) => format!("{name} failed. {error}"),
                    });
                }
                None => {}
            }
        });
    }

    fn send_server_command(&self, command: server::ServerCommand, status: &str) {
        let result = self.servers[0]
            .borrow()
            .as_ref()
            .map(|server| server.send(command));
        match result {
            Some(Ok(())) => {
                self.status.set(status.into());
                if command == server::ServerCommand::Reload {
                    self.record_log("Hot reload requested.");
                } else if command == server::ServerCommand::Restart {
                    self.record_log("Hot restart requested.");
                }
            }
            Some(Err(_)) => self
                .status
                .set("Flutter preview is no longer available.".into()),
            None => self
                .status
                .set("Start a Flutter app preview to reload or restart it.".into()),
        }
    }

    pub fn set_auto_reload(&self, enabled: bool) {
        self.auto_reload_on_save.set(enabled);
        self.record_log(if enabled {
            "Automatic reload enabled."
        } else {
            "Automatic reload disabled."
        });
        let result = self.servers[0].borrow().as_ref().map(|server| {
            server.send(server::ServerCommand::SetAutoReload(enabled))
        });
        if matches!(result, Some(Err(_))) {
            self.status
                .set("Could not update automatic reload settings.".into());
        }
    }

    pub fn hot_reload(&self) {
        self.send_server_command(server::ServerCommand::Reload, "Hot reloading…");
    }

    pub fn hot_restart(&self) {
        self.send_server_command(server::ServerCommand::Restart, "Hot restarting…");
    }

    pub fn back(&self) {
        match self.screen.get_untracked() {
            Screen::App => self.send(Input::Back),
            _ => self.screen.set(Screen::Home),
        }
    }

    pub fn clear_recents(&self) {
        self.recents.set(Vec::new());
        self.close_app();
        self.screen.set(Screen::Recents);
    }

    fn close_app(&self) {
        self.generation.update(|generation| *generation += 1);
        self.engine.borrow_mut().take();
        self.active.set(None);
        self.frame.set(None);
    }

    pub fn shutdown(&self) {
        self.engine.borrow_mut().take();
        for server in &self.servers {
            if let Some(server) = server.borrow_mut().take() {
                let _ = server.send(server::ServerCommand::Stop);
            }
        }
    }

    pub fn stop(&self) {
        self.close_app();
        self.can_reload.set(false);
        self.status.set("Preview stopped.".into());
        self.record_log("Preview stopped.");
        for (server, generation) in self.servers.iter().zip(self.server_generations)
        {
            generation.update(|generation| *generation += 1);
            if let Some(server) = server.borrow_mut().take() {
                let _ = server.send(server::ServerCommand::Stop);
            }
        }
        for server_url in self.server_urls {
            if let Some(previous) = server_url.get_untracked() {
                self.recents
                    .update(|apps| apps.retain(|app| app.url != previous));
                server_url.set(None);
            }
        }
        self.screen.set(Screen::Home);
        self.status.set(String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_report_is_bounded_and_excludes_urls_and_raw_status() {
        let session = Session::new(true);
        session.active.set(Some(App {
            url: "https://user:secret@example.test/private".into(),
            widgets: false,
        }));
        session
            .status
            .set("Sensitive source or error detail".into());
        session
            .flutter_runtime_version
            .set("Flutter 3.50.0 (stable), Dart 3.14.0".into());
        for _ in 0..60 {
            session.record_log("Flutter app preview is ready.");
        }

        let report = session.diagnostic_report();
        assert!(report.contains("Flutter web app"));
        assert!(report.contains("Dart 3.14.0"));
        assert!(report.contains("Flutter app preview is ready."));
        assert!(!report.contains("https://"));
        assert!(!report.contains("secret"));
        assert!(!report.contains("Sensitive source"));
        assert_eq!(session.logs.get_untracked().len(), 50);
    }

    #[test]
    fn widget_preview_viewport_follows_device_and_environment_settings() {
        let session = Session::new(true);
        session.active.set(Some(App {
            url: "http://localhost".into(),
            widgets: true,
        }));
        session
            .device
            .set(DeviceChoice::Preset("iphone-18-pro".into()));
        session.landscape.set(true);
        session.dark.set(true);
        session.locale.set(PreviewLocale::FrenchFrance);

        let viewport = session.viewport().unwrap();
        let device = session.device.get_untracked().device().unwrap();
        assert_eq!(
            viewport,
            Viewport::new(
                device.display,
                true,
                true,
                device.ios,
                PreviewLocale::FrenchFrance,
            )
        );
        assert!(viewport.mobile);
    }

    #[test]
    fn shutdown_does_not_update_reactive_view_state() {
        let session = Session::new(true);
        session.can_reload.set(true);
        let can_reload = session.can_reload;
        let updates = create_rw_signal(0);
        let observed_updates = updates;
        create_effect(move |_| {
            can_reload.get();
            observed_updates.update(|updates| *updates += 1);
        });
        assert_eq!(updates.get_untracked(), 1);

        session.shutdown();

        assert_eq!(updates.get_untracked(), 1);
    }
}
