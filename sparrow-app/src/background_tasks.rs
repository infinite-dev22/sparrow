//! Workspace background activity, reported safely by worker threads.
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use std::sync::mpsc::Sender;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

mod activity;
mod panel;
mod status;

pub use panel::panel;
pub use status::status;

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
