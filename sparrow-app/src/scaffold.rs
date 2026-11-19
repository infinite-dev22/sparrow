//! Dependency-aware scaffolds, previewed and applied as one reversible plan.
mod catalog;
mod manager;
pub mod engine;
mod registration;
#[cfg(test)]
mod tests;
use crate::{
    config::color::SparrowColor,
    popup_menu::{Menu, MenuItem, PopupMenuExt},
    window_tab::WindowTabData,
};
pub use catalog::Entry as Template;
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_stack, label, scroll, stack},
};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};
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
                    std::thread::spawn(move || {
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
pub fn view(window: Rc<WindowTabData>) -> impl View {
    let mode = window.scaffold.manager_mode;
    floem::views::dyn_container(move || mode.get(), move |manager| -> Box<dyn View> {
        if manager { Box::new(manager::view(window.clone())) }
        else { Box::new(scaffold_view(window.clone())) }
    }).style(|s| s.absolute().size_full())
}
fn scaffold_view(window: Rc<WindowTabData>) -> impl View {
    let state = window.scaffold.clone();
    let config = window.common.config;
    let active = state.active;
    crate::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let name = state.name;
    let template = state.template;
    let error = state.error;
    let base = state.base;
    let plan = state.plan;
    let selected = state.selected;
    let undoing = state.undoing;
    let busy = state.busy;
    let export_mode = state.export_mode;
    let picker = state.clone();
    let picker_window = window.clone();
    let auto_register = state.auto_register;
    let registration_plan = state.plan;
    let focus = crate::modal_focus::ModalFocus::default();
    let preview = state.clone();
    let w = window.clone();
    focus.set_submit(move || preview.preview(&w));
    let input = focus
        .input(name, config)
        .disabled(move || undoing.get() || busy.get() || export_mode.get());
    let id = input.id();
    let initial_focus = focus.clone();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            let exporting = export_mode.get() || undoing.get();
            let focus = initial_focus.clone();
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| {
                    if exporting {
                        focus.focus_first();
                    } else {
                        id.request_focus();
                    }
                },
            );
        }
    });
    let file_focus = focus.clone();
    let list = scroll(
        dyn_stack(
            move || {
                plan.get()
                    .map(|p| {
                        p.changes
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                (
                                    i,
                                    c.path
                                        .strip_prefix(&p.root)
                                        .unwrap_or(&c.path)
                                        .display()
                                        .to_string(),
                                    if c.after.is_none() {
                                        "Delete"
                                    } else if c.before.is_none() {
                                        "Create"
                                    } else {
                                        "Edit"
                                    },
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            },
            |(i, path, _)| (*i, path.clone()),
            move |(i, path, kind)| {
                let filename = Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                file_focus.control(
                    label(move || format!("{kind}  {filename}"))
                        .on_click_stop(move |_| selected.set(i))
                        .style(move |s| {
                            s.width_full().padding(6.0).apply_if(
                                selected.get() == i,
                                |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_CURRENT_BACKGROUND,
                                    ))
                                },
                            )
                        }),
                )
            },
        )
        .style(|s| s.flex_col()),
    )
    .style(|s| s.width(260.0).height_full());
    let diff=scroll(dyn_stack(
        move ||plan.get().map(|p|p.diff(selected.get())).unwrap_or_else(|| if export_mode.get() {"Select a bundled pack and Preview Export. Every exported file will appear here before anything is written.".into()} else {"Enter a name and select Preview Changes. Every affected file will appear here before anything is written.".into()}).lines().enumerate().map(|(i,line)|(i,line.to_string())).collect::<Vec<_>>(),
        |(i,line)|(*i,line.clone()),
        move |(_,line)| {
            let added=line.starts_with('+');let removed=line.starts_with('-');
            label(move ||line.clone()).style(move |s|s.min_width_full().height(20.0).padding_horiz(8.0).font_family(config.get().editor.font_family.clone()).font_size(12.0)
                .apply_if(added,|s|s.background(floem::peniko::Color::from_rgba8(70,160,80,35)))
                .apply_if(removed,|s|s.background(floem::peniko::Color::from_rgba8(200,60,60,35))))
        },
    ).style(|s|s.flex_col())).style(|s|s.flex_grow(1.0_f32).min_width(0.0).height_full());
    let preview = state.clone();
    let w = window.clone();
    let apply = state.clone();
    let aw = window.clone();
    let undo = state.clone();
    let history = state.history;
    let export = state.clone();
    let export_window = window.clone();
    let pub_state = state.clone();
    let pub_window = window.clone();
    let body = stack((
        label(move || {
            if undoing.get() {
                "Undo Last Scaffold".into()
            } else if export_mode.get() {
                "Export Bundled Template".into()
            } else {
                format!(
                    "New {}",
                    template.get().map(|t| t.title).unwrap_or_default()
                )
            }
        })
        .style(|s| s.font_bold()),
        label(move || base.get().display().to_string()).style(|s| s.width_full()),
        focus.control(
            label(move || {
                format!(
                    "{} ▾",
                    template
                        .get()
                        .map(|t| t.title)
                        .unwrap_or_else(|| "Choose a pack".into())
                )
            })
            .ide_popout_menu(move || {
                let mut menu = Menu::new("Scaffold packs");
                let Some(root) = picker_window.workspace.path.as_deref() else {
                    return menu;
                };
                let exporting = picker.export_mode.get_untracked();
                let entries = if exporting {
                    catalog::bundled_entries()
                } else {
                    engine::context(root, &picker.base.get_untracked())
                        .map(|ctx| catalog(&ctx.root))
                        .unwrap_or_default()
                };
                for entry in entries {
                    let state = picker.clone();
                    let base = state.base.get_untracked();
                    menu = menu.entry(
                        MenuItem::new(format!(
                            "{} · {}",
                            entry.title, entry.provenance
                        ))
                        .action(move || {
                            state.open(base.clone(), entry.clone());
                            state.export_mode.set(exporting);
                        }),
                    );
                }
                menu
            })
            .disabled(move || busy.get() || undoing.get())
                .style(move |s| s.apply_if(export_mode.get() || undoing.get(), |s| s.hide())),
        ),
        focus.control(label(|| "Manage templates…").on_click_stop({
            let window = window.clone();
            move |_| window.scaffold.open_manager(&window)
        })),
        input.style(move |s| s.apply_if(export_mode.get(), |s| s.hide())),
        focus.control(
            stack((
                crate::settings::checkbox(move || auto_register.get(), config),
                label(|| "Automatically register router and providers"),
            ))
            .style(|s| s.gap(8.0))
            .on_click_stop(move |_| {
                auto_register.update(|value| *value = !*value);
                registration_plan.set(None);
            })
            .disabled(move || busy.get())
            .style(move |s| {
                s.apply_if(
                    undoing.get()
                        || export_mode.get()
                        || !template.get().is_some_and(|t| t.registration),
                    |s| s.hide(),
                )
            }),
        ),
        scroll(label(move || {
            plan.get().map(|p| p.notes.join("\n")).unwrap_or_default()
        }))
        .style(|s| s.width_full().max_height(100.0)),
        stack((list, diff)).style(move |s| {
            s.width_full()
                .height(340.0)
                .border(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        label(move || error.get()).style(|s| s.width_full()),
        stack((
            focus.control(
                label(move || {
                    if export_mode.get() {
                        "Preview Export"
                    } else {
                        "Preview Changes"
                    }
                })
                .on_click_stop(move |_| preview.preview(&w))
                .disabled(move || busy.get()),
            ),
            focus.control(
                label(move || {
                    if undoing.get() {
                        "Undo Scaffold"
                    } else {
                        "Apply Changes"
                    }
                })
                .on_click_stop(move |_| apply.apply(&aw))
                .disabled(move || plan.get().is_none() || busy.get()),
            ),
            focus.control(
                label(|| "Undo Last Scaffold…")
                    .on_click_stop(move |_| undo.open_undo())
                    .disabled(move || history.get().is_empty()),
            ),
            focus.control(label(|| "Pub Get").on_click_stop(move |_| {
                let Some(root) = pub_window.workspace.path.as_deref() else {
                    return;
                };
                match engine::context(root, &pub_state.base.get_untracked()) {
                    Ok(ctx) => {
                        if let Some(dart) =
                            crate::dart::sdk::find_dart_binary(Some(&ctx.root))
                        {
                            pub_window.common.internal_command.send(
                                crate::command::InternalCommand::NewTerminal {
                                    profile: Some(
                                        sparrow_rpc::terminal::TerminalProfile {
                                            name: format!(
                                                "Pub Get · {}",
                                                ctx.package
                                            ),
                                            command: Some(
                                                dart.display().to_string(),
                                            ),
                                            arguments: Some(vec![
                                                "pub".into(),
                                                "get".into(),
                                            ]),
                                            workdir:
                                                lsp_types::Url::from_directory_path(
                                                    &ctx.root,
                                                )
                                                .ok(),
                                            ..Default::default()
                                        },
                                    ),
                                },
                            );
                        } else {
                            pub_state.error.set(
                                "Configure a Dart or Flutter SDK first.".into(),
                            );
                        }
                    }
                    Err(e) => pub_state.error.set(e),
                }
            })).style(move |s| s.apply_if(export_mode.get(), |s| s.hide())),
            focus.control(
                label(|| "Close").on_click_stop(move |_| active.set(false)),
            ),
        ))
        .style(|s| s.gap(16.0)),
        focus.control(
            label(|| "Export bundled default to project…")
                .on_click_stop(move |_| export.export(&export_window))
                .disabled(move || busy.get() || undoing.get())
                .style(move |s| s.apply_if(export_mode.get() || undoing.get(), |s| s.hide())),
        ),
    ))
    .style(move |s| {
        s.width(1020.0)
            .max_width_pct(95.0)
            .max_height_pct(95.0)
            .flex_col()
            .gap(12.0)
            .padding(18.0)
            .border_radius(6.0)
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .box_shadow_blur(16.0)
            .box_shadow_color(
                config.get().color(SparrowColor::SPARROW_DROPDOWN_SHADOW),
            )
            .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
            .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
    });
    let dialog=container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100).apply_if(!active.get(),|s|s.hide())).on_event_stop(EventListener::PointerDown, |_|{}).on_event(EventListener::KeyDown,move |e|{if matches!(e,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)){active.set(false);EventPropagation::Stop}else{EventPropagation::Continue}});
    focus.bind_root(dialog, active, config)
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
