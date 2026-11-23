//! Android device logs: independent streams, bounded storage and structured filters.
mod archive;
mod format;
mod gradle;
mod lifecycle;
mod model;
mod query;
mod source;
mod view;
use crate::{panel::kind::PanelKind, window_tab::WindowTabData};
use floem::{
    View,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_rw_signal,
    },
};
use model::{Buffer, Entry};
use query::Query;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Saved {
    history: Vec<String>,
    favorites: Vec<String>,
    queries: Vec<String>,
    devices: Vec<String>,
    cases: Vec<bool>,
    wraps: Vec<bool>,
    compacts: Vec<bool>,
    files: Vec<std::path::PathBuf>,
    variant: String,
    // Older TOML serializers require arrays of tables after scalar fields.
    formats: Vec<format::Formatting>,
}
impl Default for Saved {
    fn default() -> Self {
        Self {
            history: Vec::new(),
            favorites: Vec::new(),
            queries: vec!["package:mine".into()],
            devices: Vec::new(),
            cases: Vec::new(),
            wraps: Vec::new(),
            compacts: Vec::new(),
            formats: Vec::new(),
            variant: String::new(),
            files: Vec::new(),
        }
    }
}
#[derive(Clone)]
struct Pane {
    id: u64,
    query: RwSignal<String>,
    case: RwSignal<bool>,
    parsed: RwSignal<Result<Query, String>>,
    serial: RwSignal<String>,
    catalog: RwSignal<source::Catalog>,
    data: RwSignal<Buffer>,
    frozen: RwSignal<Vec<Arc<Entry>>>,
    paused: RwSignal<bool>,
    follow: RwSignal<bool>,
    wrap: RwSignal<bool>,
    compact: RwSignal<bool>,
    error: RwSignal<String>,
    epoch: Rc<Cell<u64>>,
    stream: Rc<RefCell<Option<source::Stream>>>,
    alive: Rc<Cell<bool>>,
    restart: RwSignal<u64>,
    now: RwSignal<i64>,
    format: RwSignal<format::Formatting>,
    file_path: RwSignal<Option<std::path::PathBuf>>,
    loading: RwSignal<bool>,
    tracker: Rc<RefCell<lifecycle::Tracker>>,
    visible_count: RwSignal<usize>,
    visible_packages: RwSignal<Vec<String>>,
}
impl Pane {
    fn data(id: u64, query: String) -> Self {
        Self {
            id,
            query: create_rw_signal(query),
            case: create_rw_signal(false),
            parsed: create_rw_signal(Ok(Query::All)),
            serial: create_rw_signal(String::new()),
            catalog: create_rw_signal(source::Catalog::default()),
            data: create_rw_signal(Buffer::default()),
            frozen: create_rw_signal(Vec::new()),
            paused: create_rw_signal(false),
            follow: create_rw_signal(true),
            wrap: create_rw_signal(false),
            compact: create_rw_signal(false),
            error: create_rw_signal(String::new()),
            epoch: Rc::new(Cell::new(0)),
            stream: Rc::new(RefCell::new(None)),
            alive: Rc::new(Cell::new(true)),
            restart: create_rw_signal(0),
            now: create_rw_signal(chrono::Local::now().timestamp()),
            format: create_rw_signal(format::Formatting::default()),
            file_path: create_rw_signal(None),
            loading: create_rw_signal(false),
            tracker: Rc::new(RefCell::new(lifecycle::Tracker::default())),
            visible_count: create_rw_signal(0),
            visible_packages: create_rw_signal(Vec::new()),
        }
    }
    fn new(
        id: u64,
        query: String,
        window: Rc<WindowTabData>,
        packages: RwSignal<Vec<String>>,
    ) -> Self {
        let pane = Self::data(id, query);
        clock(pane.clone());
        let p = pane.clone();
        create_effect(move |_| {
            p.parsed.set(Query::parse(&p.query.get(), p.case.get()));
        });
        let tick = create_rw_signal(0u64);
        let busy = Rc::new(Cell::new(false));
        let p = pane.clone();
        let panel = window.panel.clone();
        let scope = Scope::current();
        let reset = pane.tracker.clone();
        create_effect(move |previous: Option<Vec<String>>| {
            let current = packages.get();
            if previous.as_ref() != Some(&current) {
                *reset.borrow_mut() = Default::default();
            }
            current
        });
        create_effect(move |_| {
            tick.get();
            p.restart.get();
            let serial = p.serial.get();
            panel.styles.get();
            panel.panels.get();
            if !p.alive.get()
                || p.file_path.get().is_some()
                || busy.get()
                || !panel.is_panel_visible(&PanelKind::Logcat)
            {
                return;
            }
            busy.set(true);
            let busy = busy.clone();
            let p = p.clone();
            let expected = serial.clone();
            let send = floem::ext_event::create_ext_action(
                scope,
                move |catalog: source::Catalog| {
                    busy.set(false);
                    let Some(file) = p.file_path.try_get_untracked() else {
                        p.close();
                        return;
                    };
                    if !p.alive.get() || file.is_some() {
                        return;
                    }
                    if p.serial.get_untracked() == expected {
                        if expected.is_empty() {
                            if let Some(device) =
                                catalog.devices.iter().find(|d| d.ready)
                            {
                                p.serial.set(device.serial.clone());
                            }
                        }
                        if catalog
                            .devices
                            .iter()
                            .any(|d| d.serial == expected && d.ready)
                        {
                            let markers = p.tracker.borrow_mut().observe(
                                &expected,
                                &catalog.processes,
                                &packages.get_untracked(),
                            );
                            if !markers.is_empty() {
                                p.data.update(|buffer| {
                                    for marker in markers {
                                        buffer.push(marker);
                                    }
                                });
                            }
                        }
                        p.catalog.set(catalog);
                    }
                    let alive = p.alive.clone();
                    floem::action::exec_after(Duration::from_secs(3), move |_| {
                        if alive.get() {
                            let _ = tick.try_update(|v| *v += 1);
                        }
                    });
                },
            );
            std::thread::spawn(move || send(source::discover(&serial)));
        });
        let p = pane.clone();
        let panel = window.panel.clone();
        create_effect(move |previous| {
            let serial = p.serial.get();
            let ready = p.file_path.get().is_none()
                && p.catalog.with(|c| {
                    c.devices.iter().any(|d| d.serial == serial && d.ready)
                });
            panel.styles.get();
            panel.panels.get();
            let desired = (
                serial.clone(),
                ready,
                panel.is_panel_visible(&PanelKind::Logcat),
                p.restart.get(),
            );
            if previous.as_ref() == Some(&desired) {
                return desired;
            }
            p.epoch.set(p.epoch.get().wrapping_add(1));
            p.stream.borrow_mut().take();
            if !p.alive.get() || !ready || !desired.2 {
                return desired;
            }
            let generation = p.epoch.get();
            let changed_device =
                previous.as_ref().is_some_and(|old| old.0 != serial);
            if changed_device {
                p.data.update(Buffer::clear);
                p.frozen.set(Vec::new());
            }
            p.error.set(String::new());
            let (tx, rx) = crossbeam_channel::bounded(8);
            pump(p.clone(), generation, rx, None);
            let stream = source::Stream::start(
                super::reference_tools::adb(),
                serial,
                move |update| {
                    let _ = tx.send(update);
                },
            );
            *p.stream.borrow_mut() = Some(stream);
            desired
        });
        pane
    }
    fn load_file(
        &self,
        path: std::path::PathBuf,
        window: Rc<WindowTabData>,
        saved_filter: Option<String>,
    ) {
        self.file_path.set(Some(path.clone()));
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.stream.borrow_mut().take();
        self.catalog.set(source::Catalog::default());
        self.data.update(Buffer::clear);
        self.frozen.set(Vec::new());
        self.paused.set(false);
        self.error.set(String::new());
        self.loading.set(true);
        let generation = self.epoch.get();
        let pane = self.clone();
        let send = floem::ext_event::create_ext_action(
            window.scope,
            move |result: Result<archive::Capture, String>| {
                if !pane.alive.get()
                    || pane.epoch.get() != generation
                    || pane.loading.try_get_untracked().is_none()
                {
                    return;
                }
                pane.loading.set(false);
                match result {
                    Ok(capture) => {
                        pane.query.set(saved_filter.unwrap_or(capture.filter));
                        pane.data.set(capture.buffer);
                    }
                    Err(error) => {
                        pane.error.set(format!("Cannot import capture: {error}"))
                    }
                }
            },
        );
        let reporter = window.common.background_tasks.reporter();
        std::thread::spawn(move || {
            let _task = reporter.begin("Importing Logcat capture");
            send(archive::read(&path));
        });
    }
    fn close(&self) {
        self.alive.set(false);
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.stream.borrow_mut().take();
        let _ = self.data.try_update(Buffer::clear);
        let _ = self.frozen.try_update(Vec::clear);
    }
    fn pause(&self) {
        if !self.paused.get_untracked() {
            self.frozen.set(
                self.data
                    .with_untracked(|d| d.entries.iter().cloned().collect()),
            );
        }
        self.paused.update(|p| *p = !*p);
        if !self.paused.get_untracked() {
            self.frozen.set(Vec::new());
        }
    }
}
fn clock(pane: Pane) {
    if !pane.alive.get() {
        return;
    }
    let now = chrono::Local::now().timestamp();
    if pane.now.try_update(|value| *value = now).is_none() {
        pane.close();
        return;
    }
    floem::action::exec_after(Duration::from_secs(1), move |_| clock(pane));
}

fn pump(
    target: Pane,
    generation: u64,
    rx: crossbeam_channel::Receiver<source::Update>,
    mut previous_entry: Option<Entry>,
) {
    if !target.alive.get() || target.epoch.get() != generation {
        return;
    }
    let Some(previous_time) = target.now.try_get_untracked() else {
        target.close();
        return;
    };
    let now = chrono::Local::now().timestamp();
    if previous_time != now {
        target.now.set(now);
    }
    for update in rx.try_iter().take(8) {
        match update {
            source::Update::Lines(lines) => target.data.update(|buffer| {
                for line in lines {
                    let mut entry = if let Some(entry) = model::parse(&line) {
                        previous_entry = Some(entry.clone());
                        entry
                    } else if line.starts_with("---------") {
                        Entry {
                            message: line.clone(),
                            raw: line,
                            ..Default::default()
                        }
                    } else {
                        let mut e = previous_entry.clone().unwrap_or_default();
                        e.message = line.clone();
                        e.raw = line;
                        e
                    };
                    if let Some((package, process)) = target
                        .catalog
                        .with_untracked(|c| c.processes.get(&entry.pid).cloned())
                    {
                        entry.package = package;
                        entry.process = process;
                    }
                    buffer.push(entry);
                }
            }),
            source::Update::Error(error) => target.error.set(error),
            source::Update::End => target
                .error
                .set("Logcat stream ended. Restart capture to reconnect.".into()),
        }
    }
    floem::action::exec_after(Duration::from_millis(50), move |_| {
        pump(target, generation, rx, previous_entry)
    });
}

#[derive(Clone)]
struct State {
    window: Rc<WindowTabData>,
    scope: Scope,
    panes: RwSignal<Vec<Pane>>,
    active: RwSignal<u64>,
    next: Rc<Cell<u64>>,
    saved: RwSignal<Saved>,
    packages: RwSignal<Vec<String>>,
    model: RwSignal<gradle::Model>,
    variant: RwSignal<String>,
    syncing: RwSignal<bool>,
    gradle_error: RwSignal<String>,
    sync_cancel: Arc<AtomicBool>,
    split: RwSignal<bool>,
    import_path: RwSignal<String>,
    show_import: RwSignal<bool>,
}
impl State {
    fn new(window: Rc<WindowTabData>) -> Self {
        let saved = window
            .workspace
            .path
            .as_ref()
            .and_then(|r| {
                std::fs::read_to_string(r.join(".sparrow/logcat.toml")).ok()
            })
            .and_then(|s| toml::from_str::<Saved>(&s).ok())
            .unwrap_or_default();
        let queries = if saved.queries.is_empty() {
            vec!["package:mine".into()]
        } else {
            saved.queries.iter().take(8).cloned().collect()
        };
        let packages = create_rw_signal(Vec::<String>::new());
        let panes = queries
            .into_iter()
            .enumerate()
            .map(|(i, q)| {
                let pane = Pane::new(i as u64, q, window.clone(), packages);
                if let Some(serial) = saved.devices.get(i) {
                    pane.serial.set(serial.clone());
                }
                pane.case.set(saved.cases.get(i).copied().unwrap_or(false));
                pane.wrap.set(saved.wraps.get(i).copied().unwrap_or(false));
                let compact = saved.compacts.get(i).copied().unwrap_or(false);
                pane.compact.set(compact);
                pane.format.set(
                    saved
                        .formats
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| {
                            if compact {
                                format::Formatting::compact()
                            } else {
                                format::Formatting::default()
                            }
                        })
                        .normalized(),
                );
                if let Some(path) =
                    saved.files.get(i).filter(|p| !p.as_os_str().is_empty())
                {
                    pane.load_file(
                        path.clone(),
                        window.clone(),
                        Some(pane.query.get_untracked()),
                    );
                }
                pane
            })
            .collect::<Vec<_>>();
        let state = Self {
            scope: Scope::current(),
            packages,
            model: create_rw_signal(gradle::Model::default()),
            variant: create_rw_signal(saved.variant.clone()),
            syncing: create_rw_signal(false),
            gradle_error: create_rw_signal(String::new()),
            sync_cancel: Arc::new(AtomicBool::new(false)),
            next: Rc::new(Cell::new(panes.len() as u64)),
            panes: create_rw_signal(panes),
            active: create_rw_signal(0),
            window,
            saved: create_rw_signal(saved),
            split: create_rw_signal(false),
            import_path: create_rw_signal(String::new()),
            show_import: create_rw_signal(false),
        };
        let selected = state.clone();
        create_effect(move |_| {
            let model = selected.model.get();
            let variant = selected.variant.get();
            let args = selected.run_args();
            selected.packages.set(
                if selected.syncing.get() || !selected.gradle_error.get().is_empty()
                {
                    Vec::new()
                } else {
                    model.packages(&variant, &args)
                },
            );
        });
        if state
            .window
            .workspace
            .path
            .as_deref()
            .and_then(gradle::build_root)
            .is_some()
        {
            state.sync_gradle(false);
        }
        state
    }
    fn run_args(&self) -> Vec<String> {
        let index = self.window.active_run_config_idx.get();
        self.window.run_configs.with(|configs| {
            configs
                .get(index)
                .and_then(|c| c.args.clone())
                .unwrap_or_default()
        })
    }
    fn sync_gradle(&self, force: bool) {
        if self.syncing.get_untracked() {
            return;
        }
        let Some(workspace) = self.window.workspace.path.clone() else {
            return;
        };
        self.sync_cancel.store(false, Ordering::Release);
        self.syncing.set(true);
        self.gradle_error.set(String::new());
        let task = self.window.common.background_tasks.begin(
            "Syncing Android application variants",
            Some(self.sync_cancel.clone()),
        );
        let state = self.clone();
        let send = floem::ext_event::create_ext_action(
            self.scope,
            move |result: Result<gradle::Model, String>| {
                if state.syncing.try_get_untracked().is_none() {
                    return;
                }
                match result {
                    Ok(model) => {
                        state.model.set(model);
                        state.gradle_error.set(String::new());
                    }
                    Err(error) => {
                        state.model.set(Default::default());
                        state.gradle_error.set(error);
                    }
                }
                state.syncing.set(false);
            },
        );
        let cancel = self.sync_cancel.clone();
        std::thread::spawn(move || {
            let _task = task;
            let result = if !force {
                gradle::cached(&workspace)
                    .map(Ok)
                    .unwrap_or_else(|| gradle::sync(&workspace, &cancel))
            } else {
                gradle::sync(&workspace, &cancel)
            };
            send(result);
        });
    }
    fn remember(&self, query: &str, favorite: bool) {
        if query.trim().is_empty() {
            return;
        }
        self.saved.update(|s| {
            if favorite {
                if let Some(i) = s.favorites.iter().position(|q| q == query) {
                    s.favorites.remove(i);
                } else {
                    s.favorites.push(query.into());
                }
            } else {
                s.history.retain(|q| q != query);
                s.history.insert(0, query.into());
                s.history.truncate(30);
            }
        });
        self.save();
    }
    fn save(&self) {
        if let Some(root) = self.window.workspace.path.as_ref() {
            let mut saved = self.saved.get_untracked();
            saved.variant = self.variant.get_untracked();
            saved.queries = self.panes.with_untracked(|p| {
                p.iter().map(|p| p.query.get_untracked()).collect()
            });
            self.panes.with_untracked(|panes| {
                saved.devices =
                    panes.iter().map(|p| p.serial.get_untracked()).collect();
                saved.cases = panes.iter().map(|p| p.case.get_untracked()).collect();
                saved.wraps = panes.iter().map(|p| p.wrap.get_untracked()).collect();
                saved.formats =
                    panes.iter().map(|p| p.format.get_untracked()).collect();
                saved.files = panes
                    .iter()
                    .map(|p| p.file_path.get_untracked().unwrap_or_default())
                    .collect();
                saved.compacts =
                    panes.iter().map(|p| p.compact.get_untracked()).collect();
            });
            if let Ok(text) = toml::to_string_pretty(&saved) {
                let dir = root.join(".sparrow");
                if std::fs::create_dir_all(&dir).is_ok() {
                    let tmp = dir.join("logcat.tmp");
                    if std::fs::write(&tmp, text).is_ok() {
                        let _ = std::fs::rename(tmp, dir.join("logcat.toml"));
                    }
                }
            }
        }
    }
    fn add(&self) {
        let id = self.next.get();
        self.next.set(id + 1);
        let pane = floem::reactive::with_scope(self.scope, || {
            Pane::new(
                id,
                "package:mine".into(),
                self.window.clone(),
                self.packages,
            )
        });
        self.panes.update(|p| p.push(pane));
        self.active.set(id);
        self.save();
    }
    fn import(&self, path: std::path::PathBuf) {
        let path = if path.is_absolute() {
            path
        } else {
            self.window
                .workspace
                .path
                .as_ref()
                .map(|root| root.join(&path))
                .unwrap_or(path)
        };
        let id = self.next.get();
        self.next.set(id + 1);
        let pane = floem::reactive::with_scope(self.scope, || {
            Pane::new(id, String::new(), self.window.clone(), self.packages)
        });
        pane.load_file(path, self.window.clone(), None);
        self.panes.update(|panes| panes.push(pane));
        self.active.set(id);
        self.show_import.set(false);
        self.save();
    }
    fn remove(&self, id: u64) {
        // Cleanup updates other signals whose subscribers read the tab list.
        // Never run it while the list is mutably borrowed.
        let closing = self.panes.with_untracked(|panes| {
            panes.iter().find(|pane| pane.id == id).cloned()
        });
        if let Some(pane) = closing {
            pane.close();
        }
        self.panes
            .update(|panes| panes.retain(|pane| pane.id != id));
        if self.active.get_untracked() == id {
            if let Some(first) =
                self.panes.with_untracked(|p| p.first().map(|p| p.id))
            {
                self.active.set(first);
            }
        }
        self.save();
    }
}
pub fn panel(window: Rc<WindowTabData>) -> impl View {
    view::panel(State::new(window))
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn queued_capture_pump_is_safe_after_its_view_scope_is_disposed() {
        let scope = Scope::new();
        let pane =
            floem::reactive::with_scope(scope, || Pane::data(0, String::new()));
        scope.dispose();
        let (_tx, rx) = crossbeam_channel::bounded(8);
        pump(pane.clone(), 0, rx, None);
        clock(pane.clone());
        assert!(!pane.alive.get());
        // Cleanup is idempotent even after its reactive values have gone away.
        pane.close();
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    #[test]
    fn mixed_live_and_offline_tabs_round_trip_toml() {
        let saved = Saved {
            variant: ":app/prodFullRelease".into(),
            files: vec![Default::default(), "/tmp/capture.json".into()],
            queries: vec!["package:mine".into(), "level:ERROR".into()],
            formats: vec![
                format::Formatting::default(),
                format::Formatting::compact(),
            ],
            ..Default::default()
        };
        let text = toml::to_string_pretty(&saved).unwrap();
        let restored: Saved = toml::from_str(&text).unwrap();
        assert_eq!(restored.variant, saved.variant);
        assert_eq!(restored.files, saved.files);
        assert_eq!(restored.formats, saved.formats);
        assert_eq!(restored.queries, saved.queries);
    }
}
