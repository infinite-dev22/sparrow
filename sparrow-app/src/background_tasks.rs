//! Workspace background activity, reported safely by worker threads.
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{
        Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_memo,
    },
    views::{Decorators, dyn_stack, empty, label, scroll, stack},
};
use std::sync::mpsc::Sender;
use std::{
    cell::Cell,
    collections::BTreeMap,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    id: String,
    title: String,
    message: String,
    percentage: Option<u32>,
    cancellable: bool,
    cancelling: bool,
}
struct Entry {
    activity: Activity,
    cancel: Option<Arc<AtomicBool>>,
}
#[derive(Default)]
struct Store {
    next: AtomicU64,
    entries: Mutex<BTreeMap<String, Entry>>,
}

#[derive(Clone)]
pub struct BackgroundTasks {
    store: Arc<Store>,
    notify: Sender<()>,
    pub items: RwSignal<Vec<Activity>>,
    pub open: RwSignal<bool>,
    phase: RwSignal<f64>,
}
/// Thread-safe progress sink; never carries UI signals into workers.
#[derive(Clone)]
pub struct Reporter {
    store: Arc<Store>,
    notify: Sender<()>,
}
impl Reporter {
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        let (notify, _) = std::sync::mpsc::channel();
        Self {
            store: Arc::new(Store::default()),
            notify,
        }
    }
    #[cfg(test)]
    pub(crate) fn active_titles(&self) -> Vec<String> {
        self.store
            .entries
            .lock()
            .unwrap()
            .values()
            .map(|e| e.activity.title.clone())
            .collect()
    }
    pub fn begin(&self, title: impl Into<String>) -> Task {
        Task::begin(self.store.clone(), self.notify.clone(), title.into(), None)
    }
    pub fn update(
        &self,
        id: &str,
        title: Option<String>,
        message: Option<String>,
        percentage: Option<u32>,
    ) {
        let mut entries =
            self.store.entries.lock().unwrap_or_else(|e| e.into_inner());
        let entry = entries.entry(id.into()).or_insert_with(|| Entry {
            activity: Activity {
                id: id.into(),
                title: title.clone().unwrap_or_else(|| "Dart analysis".into()),
                message: String::new(),
                percentage: None,
                cancellable: false,
                cancelling: false,
            },
            cancel: None,
        });
        if let Some(title) = title {
            entry.activity.title = title;
        }
        if let Some(message) = message {
            entry.activity.message = message;
        }
        if let Some(value) = percentage {
            entry.activity.percentage = Some(value.min(100));
        }
        drop(entries);
        let _ = self.notify.send(());
    }
    pub fn finish(&self, id: &str) {
        self.store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        let _ = self.notify.send(());
    }
    pub fn finish_prefix(&self, prefix: &str) {
        self.store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|id, _| !id.starts_with(prefix));
        let _ = self.notify.send(());
    }
}
impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task").field("id", &self.id).finish()
    }
}
/// Keep this guard in the worker. Finishing, returning early, or unwinding
/// removes the activity; no reactive signals are touched by worker threads.
pub struct Task {
    id: String,
    store: Arc<Store>,
    notify: Sender<()>,
}
impl BackgroundTasks {
    pub fn new(scope: Scope) -> Self {
        let store = Arc::new(Store::default());
        let (notify, rx) = std::sync::mpsc::channel();
        let changed = floem::ext_event::create_signal_from_channel(rx);
        let items = scope.create_rw_signal(Vec::new());
        let source = store.clone();
        scope.create_effect(move |_| {
            changed.get();
            items.set(
                source
                    .entries
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .values()
                    .map(|entry| entry.activity.clone())
                    .collect(),
            );
        });
        Self {
            store,
            notify,
            items,
            open: scope.create_rw_signal(false),
            phase: scope.create_rw_signal(0.0),
        }
    }
    pub fn reporter(&self) -> Reporter {
        Reporter {
            store: self.store.clone(),
            notify: self.notify.clone(),
        }
    }
    pub fn begin(
        &self,
        title: impl Into<String>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Task {
        Task::begin(
            self.store.clone(),
            self.notify.clone(),
            title.into(),
            cancel,
        )
    }
    fn cancel(&self, id: &str) {
        let mut entries =
            self.store.entries.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = entries.get_mut(id) {
            if let Some(cancel) = &entry.cancel {
                cancel.store(true, Ordering::Relaxed);
                entry.activity.cancelling = true;
            }
        }
        let _ = self.notify.send(());
    }
}
/// A reporting handle that does not own the task's lifetime.
#[derive(Clone)]
pub struct TaskProgress {
    launch: bool,
    id: String,
    store: Arc<Store>,
    notify: Sender<()>,
}
impl TaskProgress {
    pub fn for_launch(mut self) -> Self {
        self.launch = true;
        self
    }
    pub fn output(&self, message: &str) {
        if self.launch && launch_ready(message) {
            self.finish();
        } else {
            self.report(message, None);
        }
    }
    pub fn finish(&self) {
        self.store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
        let _ = self.notify.send(());
    }
    pub fn report(&self, message: impl Into<String>, percentage: Option<u32>) {
        if let Some(entry) = self
            .store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&self.id)
        {
            entry.activity.message = message.into();
            entry.activity.percentage = percentage.map(|p| p.min(100));
        }
        let _ = self.notify.send(());
    }
}
impl Task {
    fn begin(
        store: Arc<Store>,
        notify: Sender<()>,
        title: String,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Self {
        let id = format!("task:{}", store.next.fetch_add(1, Ordering::Relaxed));
        store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.clone(),
                Entry {
                    activity: Activity {
                        id: id.clone(),
                        title,
                        message: String::new(),
                        percentage: None,
                        cancellable: cancel.is_some(),
                        cancelling: false,
                    },
                    cancel,
                },
            );
        let _ = notify.send(());
        Self { id, store, notify }
    }
    pub fn report_handle(&self) -> TaskProgress {
        TaskProgress {
            launch: false,
            id: self.id.clone(),
            store: self.store.clone(),
            notify: self.notify.clone(),
        }
    }
    pub fn report(&self, message: impl Into<String>, percentage: Option<u32>) {
        self.report_handle().report(message, percentage);
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        self.store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
        let _ = self.notify.send(());
    }
}

fn activities(window: Rc<WindowTabData>) -> Memo<Vec<Activity>> {
    create_memo(move |_| {
        let mut items = window.common.background_tasks.items.get();
        window.progresses.with(|progresses| {
            items.extend(progresses.values().map(|p| Activity {
                id: format!("lsp:{:?}", p.token),
                title: p.title.clone(),
                message: p.message.clone().unwrap_or_default(),
                percentage: p.percentage.map(|p| p.min(100)),
                cancellable: false,
                cancelling: false,
            }));
        });
        if let Some(message) = window.flutter_hot_reload.progress_message.get() {
            items.push(Activity {
                id: "flutter".into(),
                title: "Flutter".into(),
                message,
                percentage: None,
                cancellable: false,
                cancelling: false,
            });
        }
        if window.flutter_hot_reload.is_fetching_devices.get() {
            items.push(Activity {
                id: "devices".into(),
                title: "Discovering devices".into(),
                message: String::new(),
                percentage: None,
                cancellable: false,
                cancelling: false,
            });
        }
        window.flutter_test.run.with(|run| {
            if run.running {
                items.push(Activity {
                    id: "flutter-tests".into(),
                    title: "Running Flutter tests".into(),
                    message: format!("{} tests discovered", run.tests.len()),
                    percentage: None,
                    cancellable: false,
                    cancelling: false,
                });
            }
        });
        items
    })
}
fn animate(
    phase: RwSignal<f64>,
    items: Memo<Vec<Activity>>,
    generation: Rc<Cell<u64>>,
    ticket: u64,
    started: Instant,
) {
    floem::action::exec_after(Duration::from_millis(16), move |_| {
        if generation.get() == ticket
            && items
                .try_get_untracked()
                .is_some_and(|items| !items.is_empty())
        {
            phase.set(started.elapsed().as_secs_f64());
            animate(phase, items, generation, ticket, started);
        }
    });
}
mod bar;
use bar::progress_bar;

/// Match tool-owned readiness banners, not application output after launch.
/// PTY output is the fallback for CLI run; DAP uses structured events below.
pub(crate) fn launch_ready(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim().trim_start_matches(['✓', '√']).trim();
        line.starts_with("Built ")
            || line.starts_with("Flutter run key commands.")
            || line.starts_with("A Dart VM Service on ")
            || line.starts_with("The Dart VM service is listening on ")
            || line.starts_with("The Flutter DevTools debugger and profiler on ")
            || line.starts_with("Reloaded ")
            || line.starts_with("Restarted application in ")
    })
}
pub(crate) fn debug_launch(
    reporter: &Reporter,
    id: impl std::fmt::Debug,
    name: &str,
) {
    reporter.update(
        &format!("debug:{id:?}:launch"),
        Some(format!("Launching {name}")),
        None,
        None,
    );
}
pub(crate) fn debug_ready(reporter: &Reporter, id: impl std::fmt::Debug) {
    reporter.finish(&format!("debug:{id:?}:launch"));
}
pub(crate) fn debug_event(
    reporter: &Reporter,
    id: impl std::fmt::Debug,
    event: &serde_json::Value,
) {
    let prefix = format!("debug:{id:?}:");
    let body = event.get("body").unwrap_or(event);
    let kind = event["type"].as_str().unwrap_or_default();
    let key = format!("{prefix}{}", body["progressId"].as_str().unwrap_or("work"));
    match kind {
        "progressStart" | "dart.progressStart" => reporter.update(
            &key,
            Some(body["title"].as_str().unwrap_or("Debugger task").into()),
            body["message"].as_str().map(str::to_owned),
            body["percentage"].as_u64().map(|p| p.min(100) as u32),
        ),
        "progressUpdate" | "dart.progressUpdate" => reporter.update(
            &key,
            None,
            body["message"].as_str().map(str::to_owned),
            body["percentage"].as_u64().map(|p| p.min(100) as u32),
        ),
        "progressEnd" | "dart.progressEnd" => reporter.finish(&key),
        "flutter.appStarted" | "dart.debuggerUris" => debug_ready(reporter, id),
        "terminated" | "exited" => reporter.finish_prefix(&prefix),
        "output" => {
            if event["output"].as_str().is_some_and(launch_ready) {
                debug_ready(reporter, id);
            }
        }
        _ => {}
    }
}

pub fn status(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let tasks = window.common.background_tasks.clone();
    let open = tasks.open;
    let items = activities(window);
    let phase = tasks.phase;
    let generation = Rc::new(Cell::new(0));
    floem::reactive::create_effect(move |previous: Option<bool>| {
        let active = !items.get().is_empty();
        if previous != Some(active) {
            generation.set(generation.get() + 1);
            if active {
                animate(
                    phase,
                    items,
                    generation.clone(),
                    generation.get(),
                    Instant::now(),
                );
            }
        }
        if !active {
            open.set(false);
        }
        active
    });
    stack((
        label(move || {
            items.with(|items| {
                items
                    .first()
                    .map(|item| {
                        let title = if item.message.is_empty() {
                            item.title.clone()
                        } else {
                            format!("{}: {}", item.title, item.message)
                        };
                        if items.len() > 1 {
                            format!("{title} (+{})", items.len() - 1)
                        } else {
                            title
                        }
                    })
                    .unwrap_or_default()
            })
        })
        .style(|s| s.min_width(0.0).max_width(260.0).text_ellipsis()),
        progress_bar(
            move || {
                items.with(|items| items.first().and_then(|item| item.percentage))
            },
            phase,
            config,
        )
        .style(|s| s.width(100.0).height(4.0).flex_shrink(0.0_f32)),
        label(move || {
            items.with(|items| {
                items
                    .first()
                    .and_then(|item| item.percentage)
                    .map(|percent| format!("{percent}%"))
                    .unwrap_or_default()
            })
        })
        .style(move |s| {
            s.min_width(30.0).flex_shrink(0.0_f32).apply_if(
                items.with(|items| {
                    items.first().and_then(|item| item.percentage).is_none()
                }),
                |s| s.hide(),
            )
        }),
        label(|| "⌃"),
    ))
    .on_click_stop(move |_| open.update(|value| *value = !*value))
    .style(move |s| {
        s.height_full()
            .min_width(0.0)
            .max_width(390.0)
            .items_center()
            .gap(8.0)
            .padding_horiz(10.0)
            .cursor(floem::style::CursorStyle::Pointer)
            .apply_if(items.with(|items| items.is_empty()), |s| s.hide())
    })
}

pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let tasks = window.common.background_tasks.clone();
    let open = tasks.open;
    let items = activities(window);
    // One shared frame clock drives all visible bars and stops when idle.
    let view = stack((
        empty()
            .style(|s| s.absolute().size_full())
            .on_click_stop(move |_| open.set(false)),
        stack((
            stack((
                label(move || {
                    format!("Background Tasks ({})", items.with(|items| items.len()))
                })
                .style(|s| s.flex_grow(1.0_f32).font_bold()),
                label(|| "×").on_click_stop(move |_| open.set(false)),
            ))
            .style(|s| s.width_full().items_center()),
            scroll(
                dyn_stack(
                    move || items.get(),
                    |item| item.id.clone(),
                    move |item| {
                        let cancel_tasks = tasks.clone();
                        let id = item.id.clone();
                        let row_id = item.id.clone();
                        let current = create_memo(move |_| items.with(|items| items.iter().find(|a| a.id == row_id).cloned()));
                        let phase = tasks.phase;
                        stack((
                            stack((
                                label(move || current.get().map(|a| a.title).unwrap_or_default()).style(|s| {
                                    s.font_bold().min_width(0.0).text_ellipsis()
                                }),
                                label(move || current.get().map(|a| {
                                    if a.cancelling { "Cancelling…".into() }
                                    else if let Some(percent) = a.percentage { format!("{} · {percent}%", a.message) }
                                    else if a.message.is_empty() { "In progress…".into() }
                                    else { a.message }
                                }).unwrap_or_default()).style(move |s| {
                                    s.min_width(0.0).text_ellipsis().color(
                                        config.get().color(
                                            SparrowColor::PANEL_FOREGROUND_DIM,
                                        ),
                                    )
                                }),
                                progress_bar(move || current.get().and_then(|a| a.percentage), phase, config)
                                    .style(|s| s.width_full().height(4.0)),
                            ))
                            .style(|s| {
                                s.flex_col()
                                    .flex_grow(1.0_f32)
                                    .min_width(0.0)
                                    .gap(4.0)
                            }),
                            label(|| "Cancel")
                                .on_click_stop(move |_| cancel_tasks.cancel(&id))
                                .style(move |s| {
                                    s.color(
                                        config
                                            .get()
                                            .color(SparrowColor::EDITOR_LINK),
                                    )
                                    .apply_if(!current.get().is_some_and(|a| a.cancellable && !a.cancelling), |s| s.hide())
                                }),
                        ))
                        .style(|s| {
                            s.width_full().items_center().gap(12.0).padding_vert(8.0)
                        })
                    },
                )
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.width_full().max_height(300.0)),
        ))
        .on_event_stop(EventListener::PointerDown, |_| {})
        .on_click_stop(|_| {})
        .style(move |s| {
            s.absolute()
                .inset_right(12.0)
                .inset_bottom(config.get().ui.status_height() as f64 + 6.0)
                .width(420.0)
                .max_width_pct(95.0)
                .padding(14.0)
                .gap(8.0)
                .flex_col()
                .border(1.0)
                .border_radius(6.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .z_index(190)
            .apply_if(!open.get() || items.with(|items| items.is_empty()), |s| {
                s.hide()
            })
    })
    .keyboard_navigable()
    .on_event(EventListener::KeyDown, move |event| {
        if matches!(event, Event::KeyDown(key) if key.key.logical_key == Key::Named(NamedKey::Escape)) {
            open.set(false);
            EventPropagation::Stop
        } else { EventPropagation::Continue }
    });
    let id = view.id();
    floem::reactive::create_effect(move |_| {
        if open.get() {
            id.request_focus();
        }
    });
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_work_finishes_without_ending_the_running_process() {
        let reporter = Reporter::for_test();
        let task = reporter.begin("Launching app");
        let output = task.report_handle().for_launch();
        output.output("Building Linux application...");
        assert_eq!(reporter.active_titles(), vec!["Launching app"]);
        output.output("✓ Built build/linux/x64/release/bundle/app");
        assert!(reporter.active_titles().is_empty());
        output.output("Application continues running");
        assert!(reporter.active_titles().is_empty());
        let test = reporter.begin("Running tests");
        test.report_handle().output("✓ Built test executable");
        assert_eq!(reporter.active_titles(), vec!["Running tests"]);
        drop(test);
        drop(task);
        assert!(reporter.active_titles().is_empty());
    }
    #[test]
    fn release_profile_debug_web_and_reload_readiness() {
        for banner in [
            "✓ Built build/app/outputs/flutter-apk/app-release.apk",
            "Built build/linux/x64/profile/bundle/app",
            "Flutter run key commands.",
            "A Dart VM Service on Linux is available at: http://localhost/",
            "Reloaded 1 of 200 libraries in 120ms.",
            "Restarted application in 800ms.",
        ] {
            assert!(launch_ready(banner), "{banner}");
        }
        assert!(!launch_ready("Building app..."));
        assert!(!launch_ready("Failed to build app"));
    }
    #[test]
    fn debugger_readiness_does_not_hide_other_work_and_percentages_are_real() {
        use serde_json::json;
        let reporter = Reporter::for_test();
        debug_launch(&reporter, 1, "app");
        debug_event(
            &reporter,
            1,
            &json!({"type":"progressStart","body":{"progressId":"compile","title":"Compiling","percentage":10}}),
        );
        debug_event(
            &reporter,
            1,
            &json!({"type":"progressUpdate","body":{"progressId":"compile","percentage":65}}),
        );
        assert_eq!(
            reporter.store.entries.lock().unwrap()["debug:1:compile"]
                .activity
                .percentage,
            Some(65)
        );
        debug_event(
            &reporter,
            1,
            &json!({"type":"flutter.appStarted","body":{}}),
        );
        assert_eq!(reporter.active_titles(), vec!["Compiling"]);
        debug_event(
            &reporter,
            1,
            &json!({"type":"progressEnd","body":{"progressId":"compile"}}),
        );
        assert!(reporter.active_titles().is_empty());
        debug_launch(&reporter, 1, "app");
        debug_event(&reporter, 1, &json!({"type":"terminated"}));
        assert!(reporter.active_titles().is_empty());
    }
    #[test]
    fn worker_lifecycle_and_concurrent_tasks() {
        let store = Arc::new(Store::default());
        let (tx, _rx) = std::sync::mpsc::channel();
        let first = Task::begin(store.clone(), tx.clone(), "Build".into(), None);
        let second = Task::begin(store.clone(), tx, "Fetch".into(), None);
        let worker = std::thread::spawn(move || {
            first.report("Compiling", Some(150));
            first
        });
        let first = worker.join().unwrap();
        assert_eq!(
            store
                .entries
                .lock()
                .unwrap()
                .get(&first.id)
                .unwrap()
                .activity
                .percentage,
            Some(100)
        );
        drop(first);
        assert_eq!(store.entries.lock().unwrap().len(), 1);
        drop(second);
        assert!(store.entries.lock().unwrap().is_empty());
    }
    #[test]
    fn unwinding_worker_cleans_up_task() {
        let store = Arc::new(Store::default());
        let (tx, _rx) = std::sync::mpsc::channel();
        let task = Task::begin(store.clone(), tx, "Work".into(), None);
        assert!(
            std::thread::spawn(move || {
                let _task = task;
                panic!("test failure")
            })
            .join()
            .is_err()
        );
        assert!(store.entries.lock().unwrap().is_empty());
    }
}
