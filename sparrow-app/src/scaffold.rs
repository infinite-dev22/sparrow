//! Dependency-aware scaffolds, previewed and applied as one reversible plan.
mod catalog;
mod manager;
pub mod engine;
mod registration;
#[cfg(test)]
mod tests;
mod view;

pub use view::view;

use crate::window_tab::WindowTabData;
pub use catalog::Entry as Template;
use floem::{
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use std::path::{Path, PathBuf};
pub fn catalog(root: &Path) -> Vec<Template> {
    catalog::discover(root)
}
#[derive(Clone)]
pub struct State {
    pub active: RwSignal<bool>,
    base: RwSignal<PathBuf>,
    template: RwSignal<Option<Template>>,
    name: RwSignal<String>,
    error: RwSignal<String>,
    plan: RwSignal<Option<engine::Plan>>,
    planned_name: RwSignal<String>,
    selected: RwSignal<usize>,
    history: RwSignal<Vec<engine::Applied>>,
    undoing: RwSignal<bool>,
    busy: RwSignal<bool>,
    generation: RwSignal<u64>,
    auto_register: RwSignal<bool>,
    export_mode: RwSignal<bool>,
    manager: manager::State,
    manager_mode: RwSignal<bool>,
}
impl State {
    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            base: cx.create_rw_signal(PathBuf::new()),
            template: cx.create_rw_signal(None),
            name: cx.create_rw_signal(String::new()),
            error: cx.create_rw_signal(String::new()),
            plan: cx.create_rw_signal(None),
            planned_name: cx.create_rw_signal(String::new()),
            selected: cx.create_rw_signal(0),
            history: cx.create_rw_signal(Vec::new()),
            undoing: cx.create_rw_signal(false),
            busy: cx.create_rw_signal(false),
            generation: cx.create_rw_signal(0),
            auto_register: cx.create_rw_signal(true),
            export_mode: cx.create_rw_signal(false),
            manager: manager::State::new(cx),
            manager_mode: cx.create_rw_signal(false),
        }
    }
    pub fn open(&self, base: PathBuf, template: Template) {
        self.manager_mode.set(false);
        self.generation.update(|g| *g += 1);
        self.busy.set(false);
        self.export_mode.set(false);
        self.base.set(base);
        self.template.set(Some(template));
        self.name.set(String::new());
        self.error.set(String::new());
        self.plan.set(None);
        self.selected.set(0);
        self.undoing.set(false);
        self.active.set(true);
    }
    pub fn open_catalog(&self, window: &WindowTabData, exporting: bool) {
        self.manager_mode.set(false);
        let Some(root) = window.workspace.path.as_ref() else {
            return;
        };
        let base = window
            .main_split
            .active_editor
            .get_untracked()
            .and_then(|e| {
                e.doc().content.with_untracked(|c| {
                    c.path().and_then(|p| p.parent()).map(Path::to_path_buf)
                })
            })
            .unwrap_or_else(|| root.clone());
        let result = engine::context(root, &base).and_then(|ctx| {
            let entries = if exporting {
                catalog::bundled_entries()
            } else {
                catalog(&ctx.root)
            };
            entries
                .into_iter()
                .next()
                .ok_or_else(|| "No scaffold packs available".to_string())
        });
        match result {
            Ok(entry) => {
                self.open(base, entry);
                self.export_mode.set(exporting);
            }
            Err(e) => {
                self.error.set(e);
                self.active.set(true);
            }
        }
    }
    pub fn open_feature(&self, window: &WindowTabData) {
        self.manager_mode.set(false);
        if let Some(root) = window.workspace.path.clone() {
            let base = window
                .main_split
                .active_editor
                .get_untracked()
                .and_then(|e| {
                    e.doc().content.with_untracked(|c| {
                        c.path().and_then(|p| p.parent()).map(Path::to_path_buf)
                    })
                })
                .unwrap_or_else(|| root.clone());
            let result = engine::context(&root, &base)
                .and_then(|ctx| catalog::feature(&ctx.root));
            match result {
                Ok(entry) => self.open(base, entry),
                Err(error) => {
                    self.base.set(base);
                    self.plan.set(None);
                    self.template.set(None);
                    self.error.set(error);
                    self.active.set(true);
                }
            }
        }
    }
    pub fn open_undo(&self) {
        self.manager_mode.set(false);
        self.generation.update(|g| *g += 1);
        self.busy.set(false);
        self.export_mode.set(false);
        self.undoing.set(true);
        self.selected.set(0);
        self.error.set(String::new());
        self.plan
            .set(self.history.get_untracked().last().map(|a| a.undo_plan()));
        self.active.set(true);
        if self.plan.get_untracked().is_none() {
            self.error
                .set("No scaffold to undo in this window session.".into());
        }
    }
    fn preview(&self, window: &WindowTabData) {
        if self.busy.get_untracked() {
            return;
        }
        self.error.set(String::new());
        self.plan.set(None);
        self.selected.set(0);
        if self.undoing.get_untracked() {
            self.open_undo();
            return;
        }
        if self.export_mode.get_untracked() {
            self.export(window);
            return;
        }
        let mut registration_context = None;
        let result = (|| {
            let t = self.template.get_untracked().ok_or("Choose a scaffold")?;
            let root = window.workspace.path.as_deref().ok_or("Open a project")?;
            let ctx = engine::context(root, &self.base.get_untracked())?;
            let name = self.name.get_untracked();
            let pack = t.load()?;
            let mut plan = match pack.target {
                engine::Target::Package => engine::plan_pack(&ctx, &pack, &name)?,
                engine::Target::Sibling => engine::plan_sibling(
                    &ctx,
                    &self.base.get_untracked(),
                    &pack,
                    &name,
                )?,
            };
            plan.notes.insert(0, format!("Source: {}", t.provenance));
            if pack.registration.is_some() && self.auto_register.get_untracked() {
                if window.main_split.docs.with_untracked(|docs| {
                    docs.iter().any(|(path, doc)| {
                        path.starts_with(&ctx.root)
                            && crate::dart::is_dart_path(path)
                            && !doc.is_pristine()
                    })
                }) {
                    return Err("Save modified Dart files before analyzer-backed registration.".into());
                }
                registration_context = Some(ctx);
            }
            Ok(plan)
        })();
        match result {
            Ok(plan) => {
                if let Some(ctx) = registration_context {
                    self.busy.set(true);
                    self.error.set("Resolving router/provider registrations with the Dart analyzer… First use may download the bundled analyzer's pinned dependencies.".into());
                    self.generation.update(|g| *g += 1);
                    let ticket = self.generation.get_untracked();
                    let state = self.clone();
                    let name = self.name.get_untracked();
                    let planned_name = name.clone();
                    let send = floem::ext_event::create_ext_action(
                        window.scope,
                        move |result: Result<engine::Plan, String>| {
                            if state.generation.get_untracked() != ticket
                                || !state.active.get_untracked()
                            {
                                return;
                            }
                            state.busy.set(false);
                            state.error.set(String::new());
                            match result {
                                Ok(plan) => {
                                    state.planned_name.set(planned_name);
                                    state.plan.set(Some(plan));
                                }
                                Err(error) => state.error.set(error),
                            }
                        },
                    );
                    let task = window.common.background_tasks.begin("Resolving scaffold registrations", None);
                    std::thread::spawn(move || {
                        let _task = task;
                        send(registration::augment(plan, &ctx, &name))
                    });
                } else {
                    self.planned_name.set(self.name.get_untracked());
                    self.plan.set(Some(plan));
                }
            }
            Err(error) => self.error.set(error),
        }
    }
    fn export(&self, window: &WindowTabData) {
        if self.busy.get_untracked() {
            return;
        }
        let result = (|| {
            let entry = self.template.get_untracked().ok_or("Choose a scaffold")?;
            let root = window.workspace.path.as_deref().ok_or("Open a project")?;
            let ctx = engine::context(root, &self.base.get_untracked())?;
            engine::plan_export(&ctx, catalog::export_files(&entry.id)?)
        })();
        match result {
            Ok(plan) => {
                self.planned_name.set(self.name.get_untracked());
                self.selected.set(0);
                self.plan.set(Some(plan));
                self.error.set(String::new());
            }
            Err(e) => self.error.set(e),
        }
    }
    fn apply(&self, window: &WindowTabData) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(plan) = self.plan.get_untracked() else {
            self.error
                .set("Preview the scaffold before applying.".into());
            return;
        };
        if !self.undoing.get_untracked()
            && self.planned_name.get_untracked() != self.name.get_untracked()
        {
            self.error
                .set("The name changed. Preview again before applying.".into());
            return;
        }
        if plan.changes.is_empty() {
            self.error.set("No changes to apply.".into());
            return;
        }
        if window.main_split.docs.with_untracked(|docs| {
            plan.changes
                .iter()
                .any(|c| docs.get(&c.path).is_some_and(|d| !d.is_pristine()))
        }) {
            self.error.set(
                "Save modified files in this plan before applying or undoing."
                    .into(),
            );
            return;
        }
        let undoing = self.undoing.get_untracked();
        let result = if undoing {
            self.history
                .get_untracked()
                .last()
                .ok_or_else(|| "No scaffold to undo".to_string())
                .and_then(|a| a.undo())
        } else {
            plan.apply()
                .map(|applied| self.history.update(|h| h.push(applied)))
        };
        match result {
            Err(error) => self.error.set(error),
            Ok(()) => {
                if undoing {
                    self.history.update(|h| {
                        h.pop();
                    });
                }
                for change in &plan.changes {
                    if let Some(after) = &change.after {
                        window.main_split.docs.with_untracked(|docs| {
                            if let Some(doc) = docs.get(&change.path) {
                                doc.reload(
                                    lapce_xi_rope::Rope::from(after.clone()),
                                    true,
                                );
                            }
                        });
                    } else {
                        let editors = window
                            .main_split
                            .editors
                            .with_editors_untracked(|items| {
                                items
                                    .values()
                                    .filter(|e| {
                                        e.doc().content.with_untracked(|c| {
                                            c.path() == Some(&change.path)
                                        })
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>()
                            });
                        for editor in editors {
                            if let Some(tab) = editor.editor_tab_id.get_untracked() {
                                window.main_split.editor_tab_child_close(
                                    tab,
                                    crate::editor_tab::EditorTabChild::Editor(
                                        editor.id(),
                                    ),
                                    true,
                                );
                            }
                        }
                        window.main_split.docs.update(|docs| {
                            docs.remove(&change.path);
                        });
                        window.common.dart_lsp.send_close(change.path.clone());
                    }
                }
                window.file_explorer.reload();
                self.plan.set(None);
                self.error.set(format!("{} {} file(s). {}",if undoing{"Restored"}else{"Applied"},plan.changes.len(),if undoing{""}else{"Undo Last Scaffold is available here and in the command palette. Run Pub Get if dependencies changed."}));
            }
        }
    }
}
#[cfg(test)]
mod pack_tests;
#[cfg(test)]
mod sdk_validation {
    use super::*;
    #[test]
    #[ignore = "requires isolated Flutter project through SPARROW_SCAFFOLD_EXPORT_ROOT"]
    fn export_templates_for_installed_sdk_analysis() {
        let root = PathBuf::from(
            std::env::var_os("SPARROW_SCAFFOLD_EXPORT_ROOT").expect("fixture root"),
        );
        let ctx = engine::context(&root, &root).unwrap();
        for entry in catalog(&root) {
            let pack = entry.load().unwrap();
            if pack.target == engine::Target::Sibling {
                let base = root.join("lib/generated").join(&pack.id);
                std::fs::create_dir_all(&base).unwrap();
                engine::plan_sibling(&ctx, &base, &pack, "Counter")
                    .unwrap()
                    .apply()
                    .unwrap();
            }
        }
    }
}
