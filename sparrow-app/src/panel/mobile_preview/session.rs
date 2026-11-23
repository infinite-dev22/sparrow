//! Reactive state and lifetime of a simulated device.
use std::{cell::RefCell, path::PathBuf, rc::Rc};

use crossbeam_channel::Sender;
use floem::{
    ext_event::create_signal_from_channel,
    peniko::Image,
    reactive::{RwSignal, SignalGet, SignalUpdate, create_effect, create_rw_signal},
};

use super::{
    device::{DEVICES, Display, Viewport},
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
    pub device: RwSignal<usize>,
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
    pub bars: RwSignal<bool>,
    pub panel_visible: RwSignal<bool>,
    bar_sequence: RwSignal<u64>,
    generation: RwSignal<u64>,
    server_generations: [RwSignal<u64>; 2],
    server_urls: [RwSignal<Option<String>>; 2],
    engine: Rc<RefCell<Option<Sender<Input>>>>,
    servers: [Rc<RefCell<Option<Sender<()>>>>; 2],
    pub files: Rc<RefCell<Option<tempfile::TempDir>>>,
    pub files_revision: RwSignal<u64>,
}

impl Session {
    pub fn new() -> Self {
        let session = Self {
            device: create_rw_signal(0),
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

    pub fn display(&self) -> Option<Display> {
        if let Some(device) = DEVICES.get(self.device.get()) {
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
        if self.active.get().is_some_and(|entry| entry.widgets) {
            return Some(Viewport {
                size: (800, 760),
                scale: 1.0,
                landscape: false,
                dark,
                ios: false,
                mobile: false,
            });
        }
        Some(Viewport::new(
            self.display()?,
            self.landscape.get(),
            dark,
            DEVICES
                .get(self.device.get())
                .is_some_and(|device| device.ios),
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

    pub fn start_server(&self, flutter: PathBuf, root: PathBuf, mode: Mode) {
        let slot = usize::from(mode == Mode::Widgets);
        let server = self.servers[slot].clone();
        let generation_signal = self.server_generations[slot];
        let server_url = self.server_urls[slot];
        server.borrow_mut().take();
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
        let (stop, updates) = server::start(flutter, root, mode);
        server.replace(Some(stop));
        let updates = create_signal_from_channel(updates);
        let session = self.clone();
        create_effect(move |_| {
            if generation_signal.get_untracked() != generation {
                return;
            }
            match updates.get() {
                Some(server::Event::Ready(url)) => {
                    server_url.set(Some(url.clone()));
                    session.open(App {
                        url,
                        widgets: mode == Mode::Widgets,
                    });
                }
                Some(server::Event::Failed(error)) => {
                    server.borrow_mut().take();
                    session.status.set(error);
                }
                None => {}
            }
        });
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
            server.borrow_mut().take();
        }
    }

    pub fn stop(&self) {
        self.close_app();
        for (server, generation) in self.servers.iter().zip(self.server_generations)
        {
            generation.update(|generation| *generation += 1);
            server.borrow_mut().take();
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
