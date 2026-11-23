use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use floem::{
    ext_event::create_ext_action,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
};

use super::{Issue, Options, Tab};
use crate::window_tab::WindowTabData;

#[derive(Clone, PartialEq)]
pub(super) enum Report {
    Closed,
    Running,
    Finished(Arc<Vec<Issue>>),
    Failed(String),
    Cancelled,
}

#[derive(Clone, Copy)]
pub(crate) struct Inspections {
    pub(super) report: RwSignal<Report>,
    pub(super) tab: RwSignal<Tab>,
    pub(super) action_request: RwSignal<u64>,
    pub(super) options: Options,
    pub(super) excluded: RwSignal<Vec<Issue>>,
    worker: RwSignal<Option<Arc<AtomicBool>>>,
}

impl Inspections {
    pub(crate) fn new(scope: Scope) -> Self {
        Self {
            report: scope.create_rw_signal(Report::Closed),
            tab: scope.create_rw_signal(Tab::File),
            action_request: scope.create_rw_signal(0),
            options: Options::new(scope),
            excluded: scope.create_rw_signal(Vec::new()),
            worker: scope.create_rw_signal(None),
        }
    }

    pub(crate) fn start(self, window: &WindowTabData) {
        self.cancel();
        self.excluded.set(Vec::new());
        self.tab.set(Tab::Inspection);
        let Some(root) = window
            .workspace
            .path
            .clone()
            .filter(|_| window.workspace.kind.is_local())
        else {
            self.report.set(Report::Failed(
                "Open a local workspace to inspect its saved files.".into(),
            ));
            return;
        };

        let cancel = Arc::new(AtomicBool::new(false));
        self.worker.set(Some(cancel.clone()));
        self.report.set(Report::Running);
        let id = cancel.clone();
        let complete = create_ext_action(window.scope, move |result| {
            if !self
                .worker
                .try_get_untracked()
                .flatten()
                .is_some_and(|current| Arc::ptr_eq(&current, &id))
            {
                return;
            }
            self.worker.set(None);
            self.report.set(match result {
                Ok(Some(results)) => Report::Finished(Arc::new(results)),
                Ok(None) => Report::Cancelled,
                Err(error) => Report::Failed(error),
            });
        });
        let task = window
            .common
            .background_tasks
            .begin("Inspecting project", Some(cancel.clone()));
        let result = std::thread::Builder::new()
            .name("ProjectInspection".into())
            .spawn(move || {
                let _task = task;
                let result = crate::dart::analyzer::inspect_workspace(
                    &root, &cancel,
                )
                .map(|report| {
                    report.map(|files| {
                        files
                            .into_iter()
                            .flat_map(|(path, diagnostics)| {
                                diagnostics.into_iter().map(move |diagnostic| {
                                    Issue {
                                        path: path.clone(),
                                        diagnostic,
                                        offset: None,
                                    }
                                })
                            })
                            .collect()
                    })
                });
                complete(result);
            });
        if let Err(error) = result {
            self.worker.set(None);
            self.report.set(Report::Failed(format!(
                "Could not start project inspection: {error}"
            )));
        }
    }

    pub(crate) fn cancel(self) {
        if let Some(worker) = self.worker.get_untracked() {
            worker.store(true, Ordering::Relaxed);
        }
        self.worker.set(None);
    }

    pub(super) fn close(self) {
        self.cancel();
        self.excluded.set(Vec::new());
        self.report.set(Report::Closed);
        self.tab.set(Tab::Project);
    }
}
