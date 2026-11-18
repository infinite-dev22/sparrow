//! In-memory pack editing with the generator's strict renderer and transactional saves.
use super::{
    boundary, catalog, copy_files, editable, engine, pack_from_files, stage,
    valid_id,
};
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    views::{
        Decorators, container, dyn_container, dyn_stack,
        editor::{
            text::{Document, SimpleStylingBuilder},
            text_document::TextDocument,
        },
        label, scroll, stack, text_editor,
    },
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

type Rendered = Result<Vec<(PathBuf, String)>, String>;

#[derive(Clone, Debug)]
pub(super) struct Draft {
    files: BTreeMap<PathBuf, String>,
    original: BTreeMap<PathBuf, String>,
    directory: PathBuf,
    create_only: bool,
    ctx: engine::Context,
    source: String,
}
impl Draft {
    fn load(
        entry: &catalog::Entry,
        root: &Path,
        destination: &Path,
    ) -> Result<Self, String> {
        let ctx = engine::context(root, root)?;
        let (files, directory, create_only) = match entry.manifest() {
            Some(manifest) if manifest.file_name().is_some_and(|n|n=="pack.yaml") => {
                let manifest = editable(entry,root)?;
                (copy_files(entry)?,manifest.parent().unwrap().to_path_buf(),false)
            }
            Some(_) => return Err("Duplicate this legacy template into a pack first, then open Template Editor.".into()),
            None => {
                valid_id(&entry.id)?;
                (copy_files(entry)?,destination.join(&entry.id),true)
            }
        };
        let files = files.into_iter().collect::<BTreeMap<_, _>>();
        Ok(Self {
            original: files.clone(),
            files,
            directory,
            create_only,
            ctx,
            source: entry.provenance.clone(),
        })
    }
    pub(super) fn dirty(&self) -> bool {
        self.files != self.original
    }
    fn validate(&self, sample: &str) -> Rendered {
        let files = self
            .files
            .iter()
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect::<Vec<_>>();
        let pack = pack_from_files(&files)?;
        valid_id(&pack.id)?;
        // Disabled packs can still be inspected and edited without enabling them.
        engine::render_pack(&self.ctx, &pack, sample)
    }
    fn save_plan(&self, sample: &str) -> Result<engine::Plan, String> {
        self.validate(sample)?;
        if !self.dirty() && !self.create_only {
            return Err("No draft changes to save".into());
        }
        let paths = self
            .original
            .keys()
            .chain(self.files.keys())
            .collect::<std::collections::BTreeSet<_>>();
        let changes = paths
            .into_iter()
            .map(|p| (self.directory.join(p), self.files.get(p).cloned()))
            .collect();
        let inputs = if self.create_only {
            vec![]
        } else {
            self.original
                .keys()
                .chain(self.files.keys())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|p| (self.directory.join(p), self.original.get(p).cloned()))
                .collect()
        };
        engine::management_plan(
            &boundary(&self.directory)?,
            "Save template edits",
            changes,
            self.create_only,
        )?
        .with_management_inputs(inputs)
    }
    fn add_file(&mut self, path: &str) -> Result<PathBuf, String> {
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("Use a relative file path without traversal".into());
        }
        if self.files.contains_key(&path) {
            return Err("This draft file already exists".into());
        }
        if self.files.len() >= 256 {
            return Err("A pack may contain at most 256 files".into());
        }
        self.files.insert(path.clone(), String::new());
        Ok(path)
    }
    fn remove_file(&mut self, path: &Path) -> Result<(), String> {
        if path == Path::new("pack.yaml") {
            return Err("The pack manifest cannot be removed".into());
        }
        self.files.remove(path).ok_or("Select a draft file")?;
        Ok(())
    }
    fn mark_saved(&mut self) {
        self.original = self.files.clone();
        self.create_only = false;
        self.source = format!("Editable: {}", self.directory.display());
    }
}

#[derive(Clone)]
pub(super) struct State {
    pub open: RwSignal<bool>,
    pub draft: RwSignal<Option<Draft>>,
    file: RwSignal<PathBuf>,
    sample: RwSignal<String>,
    new_file: RwSignal<String>,
    rendered: RwSignal<Rendered>,
    epoch: RwSignal<u64>,
    revision: RwSignal<u64>,
}
impl State {
    pub fn new(cx: Scope) -> Self {
        Self {
            open: cx.create_rw_signal(false),
            draft: cx.create_rw_signal(None),
            file: cx.create_rw_signal("pack.yaml".into()),
            sample: cx.create_rw_signal("Example".into()),
            new_file: cx.create_rw_signal("templates/new.dart.tmpl".into()),
            rendered: cx.create_rw_signal(Err("Validating…".into())),
            epoch: cx.create_rw_signal(0),
            revision: cx.create_rw_signal(0),
        }
    }
}
pub(super) fn open(window: &WindowTabData) -> Result<(), String> {
    let manager = &window.scaffold.manager;
    let editor = &manager.editor;
    if !editor.draft.get_untracked().is_some_and(|d| d.dirty()) {
        let entry = manager.entry().ok_or("Select a template pack")?;
        let draft = Draft::load(
            &entry,
            &manager.root.get_untracked(),
            &manager.destination()?,
        )?;
        editor.draft.set(Some(draft));
        editor.file.set("pack.yaml".into());
        editor.epoch.update(|v| *v += 1);
    }
    window.scaffold.plan.set(None);
    window.scaffold.error.set(String::new());
    editor.open.set(true);
    Ok(())
}

pub(super) fn view(window: Rc<WindowTabData>) -> impl View {
    let parent = window.scaffold.clone();
    let state = parent.manager.editor.clone();
    let open = state.open;
    let active = parent.active;
    let draft = state.draft;
    let selected = state.file;
    let sample = state.sample;
    let new_file = state.new_file;
    let rendered = state.rendered;
    let epoch = state.epoch;
    let revision = state.revision;
    let plan = parent.plan;
    let error = parent.error;
    let config = window.common.config;
    let focus = crate::modal_focus::ModalFocus::default();
    crate::modal_focus::restore_workbench_on_close(active, window.common.clone());
    floem::reactive::create_effect(move |_| {
        let current = draft.get();
        let sample = sample.get();
        revision.update(|r| *r += 1);
        let token = revision.get_untracked();
        rendered.set(Err("Validating…".into()));
        plan.set(None);
        floem::action::exec_after(
            std::time::Duration::from_millis(150),
            move |_| {
                if revision.get_untracked() == token && open.get_untracked() {
                    rendered.set(
                        current
                            .ok_or("No draft".to_string())
                            .and_then(|d| d.validate(&sample)),
                    );
                }
            },
        );
    });
    let file_focus = focus.clone();
    let files = scroll(
        dyn_stack(
            move || {
                draft
                    .get()
                    .map(|d| d.files.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            },
            |p| p.clone(),
            move |path| {
                let label_path = path.clone();
                let click_path = path.clone();
                file_focus.control(
                    label(move || label_path.display().to_string())
                        .on_click_stop(move |_| selected.set(click_path.clone()))
                        .style(move |s| {
                            s.padding(6.0).width_full().apply_if(
                                selected.get() == path,
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
    .style(|s| s.width(210.0).min_width(140.0).height_full());
    let editor_focus = focus.clone();
    let documents =
        Rc::new(RefCell::new(BTreeMap::<PathBuf, Rc<TextDocument>>::new()));
    let editor_scope = Scope::current();
    let text = dyn_container(
        move || (epoch.get(), selected.get()),
        move |(_, path)| {
            let body = draft
                .get_untracked()
                .and_then(|d| d.files.get(&path).cloned())
                .unwrap_or_default();
            // Keep document undo history when switching file tabs. Documents
            // belong to the editor scope, rather than the disposable file view.
            let cached = documents
                .borrow()
                .get(&path)
                .cloned()
                .filter(|doc| doc.text().to_string() == body);
            let doc = cached.unwrap_or_else(|| {
                let doc = Rc::new(TextDocument::new(editor_scope, body));
                let update_path = path.clone();
                doc.add_on_update(move |event| {
                    let Some(editor) = event.editor else {
                        return;
                    };
                    let text = editor.doc().text().to_string();
                    plan.set(None);
                    draft.update(|d| {
                        if let Some(d) = d {
                            d.files.insert(update_path.clone(), text);
                        }
                    });
                    error.set(String::new());
                });
                documents.borrow_mut().insert(path.clone(), doc.clone());
                doc
            });
            let style = SimpleStylingBuilder::default()
                .font_size(13)
                .font_family(vec![
                    floem::text::FamilyOwned::Name(
                        config.get_untracked().editor.font_family.clone(),
                    ),
                    floem::text::FamilyOwned::Monospace,
                ])
                .build();
            let editor = text_editor("")
                .use_doc(doc)
                .styling(style)
                .editor_style(move |s| {
                    s.cursor_color(config.get().color(SparrowColor::EDITOR_CARET))
                        .selection_color(
                            config.get().color(SparrowColor::EDITOR_SELECTION),
                        )
                        .current_line_color(
                            config.get().color(SparrowColor::EDITOR_CURRENT_LINE),
                        )
                })
                .style(move |s| {
                    s.size_full()
                        .font_family(config.get().editor.font_family.clone())
                        .font_size(13.0)
                        .background(
                            config.get().color(SparrowColor::EDITOR_BACKGROUND),
                        )
                });
            editor_focus.control(editor)
        },
    )
    .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full());
    let output = scroll(
        dyn_stack(
            move || {
                let mut text = match rendered.get() {
                    Err(e) => e,
                    Ok(files) => files
                        .iter()
                        .map(|(p, s)| format!("── {} ──\n{}\n", p.display(), s))
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                if let Some(p) = plan.get() {
                    text = format!(
                        "SAVE DIFF · {}\n{}\n",
                        p.title,
                        (0..p.changes.len())
                            .map(|i| p.diff(i))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                }
                let mut lines = text
                    .lines()
                    .take(2000)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if text.lines().count() > 2000 {
                    lines.push("Preview display limited to 2,000 lines.".into());
                }
                lines.into_iter().enumerate().collect::<Vec<_>>()
            },
            |(i, s)| (*i, s.clone()),
            move |(_, text)| {
                label(move || text.clone()).style(move |s| {
                    s.height(20.0)
                        .padding_horiz(6.0)
                        .font_family(config.get().editor.font_family.clone())
                        .font_size(12.0)
                })
            },
        )
        .style(|s| s.flex_col()),
    )
    .style(|s| s.width(420.0).min_width(250.0).height_full());
    let add_window = window.clone();
    let remove_window = window.clone();
    let save_window = window.clone();
    let apply_window = window.clone();
    let discard_window = window.clone();
    let body = stack((
        label(move ||format!("Template Editor{}",if draft.get().is_some_and(|d|d.dirty()) {" · Unsaved draft"} else {""}))
            .style(|s|s.font_bold()),
        label(move ||draft.get().map(|d|format!("{} → {}",d.source,d.directory.display())).unwrap_or_default()),
        label(||"Edit metadata in pack.yaml. Live preview renders generated files without changing your project."),
        stack((
            label(||"Sample name:"),focus.input(sample).style(|s|s.width(180.0).height(28.0)),
            label(move ||match rendered.get() {Ok(files)=>format!("Pack valid · {} generated files",files.len()),Err(e)=>e})
                .style(move |s|s.flex_grow(1.0_f32).min_width(0.0).apply_if(rendered.get().is_err(),|s|s.color(config.get().color(SparrowColor::SPARROW_ERROR)))) ,
        )).style(|s|s.gap(12.0).items_center()),
        stack((files,text,output)).style(|s|s.width_full().height(430.0).gap(10.0)),
        stack((
            focus.input(new_file).style(|s|s.width(270.0).height(28.0)),
            focus.control(label(||"Add File").on_click_stop(move |_| {
                let mut result = Err("No draft".to_string());
                draft.update(|d| {if let Some(d)=d {result=d.add_file(&new_file.get_untracked());}});
                match result {Ok(path)=>selected.set(path),Err(e)=>add_window.scaffold.error.set(e)}
            })),
            focus.control(label(||"Remove File").on_click_stop(move |_| {
                let mut result = Err("No draft".to_string());
                draft.update(|d| {if let Some(d)=d {result=d.remove_file(&selected.get_untracked());}});
                match result {Ok(())=>selected.set("pack.yaml".into()),Err(e)=>remove_window.scaffold.error.set(e)}
            })),
        )).style(|s|s.gap(16.0).items_center()),
        label(move ||error.get()).style(|s|s.width_full()),
        stack((
            focus.control(label(||"Preview Save").on_click_stop(move |_| {
                let result = draft.get_untracked().ok_or("No draft".into()).and_then(|d|d.save_plan(&sample.get_untracked()));
                save_window.scaffold.undoing.set(false);
                stage(&save_window,result);
            })),
            focus.control(label(||"Apply Save").disabled(move ||plan.get().is_none()).on_click_stop(move |_| {
                apply_window.scaffold.apply(&apply_window);
                if plan.get_untracked().is_none() {
                    draft.update(|d| {if let Some(d)=d {d.mark_saved();}});
                    apply_window.scaffold.manager.refresh();
                    error.set("Template saved. Undo Last Change is available in the manager.".into());
                }
            })),
            focus.control(label(||"Generated Preview").on_click_stop(move |_|plan.set(None))),
            focus.control(label(||"Back to Manager").on_click_stop(move |_| {plan.set(None);open.set(false);})),
            focus.control(label(||"Discard Draft").on_click_stop(move |_| {
                draft.set(None);plan.set(None);open.set(false);
                discard_window.scaffold.error.set("Draft discarded; saved files are unchanged.".into());
            })),
        )).style(|s|s.gap(18.0)),
    )).style(move |s|s.width(1380.0).max_width_pct(96.0).max_height_pct(96.0).flex_col()
        .gap(10.0).padding(18.0).border(1.0).border_radius(6.0)
        .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        .color(config.get().color(SparrowColor::EDITOR_FOREGROUND)));
    let dialog = container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100)
        .apply_if(!active.get(),|s|s.hide()))
        .on_event_stop(EventListener::PointerDown,|_|{})
        .on_event(EventListener::KeyDown,move |event| {
            if matches!(event,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)) {
                plan.set(None);open.set(false);EventPropagation::Stop
            } else {EventPropagation::Continue}
        });
    focus.bind_root(dialog, active, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("pubspec.yaml"),
            "name: editor_test
",
        )
        .unwrap();
        d
    }
    fn local(root: &Path) -> Draft {
        let destination = root.join(".sparrow/scaffolds");
        super::super::create(&destination, "team.example")
            .unwrap()
            .apply()
            .unwrap();
        let entry = catalog::manager_entries(root)
            .into_iter()
            .find(|e| e.id == "team.example")
            .unwrap();
        Draft::load(&entry, root, &destination).unwrap()
    }
    fn body(d: &mut Draft, text: &str) {
        d.files
            .insert("templates/template.dart.tmpl".into(), text.into());
    }
    #[test]
    fn live_validation_checks_yaml_sources_metadata_variables_and_output_paths() {
        let root = project();
        let original = local(root.path());
        for invalid in [
            "not yaml: [",
            "schema: 99
id: bad
version: 1.0.0
title: Bad
files: []",
        ] {
            let mut d = original.clone();
            d.files.insert("pack.yaml".into(), invalid.into());
            assert!(d.validate("Counter").is_err());
            assert!(d.save_plan("Counter").is_err());
        }
        for invalid in ["{{unknown}}", "{{name", "{{ name }} {{missing}}"] {
            let mut d = original.clone();
            body(&mut d, invalid);
            assert!(d.validate("Counter").is_err());
        }
        let mut d = original.clone();
        d.files.remove(Path::new("templates/template.dart.tmpl"));
        assert!(
            d.validate("Counter")
                .unwrap_err()
                .contains("Missing template source")
        );
        let mut d = original.clone();
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("{{snake}}.dart", "../escape.dart");
        assert!(d.validate("Counter").is_err());
        assert!(original.validate("lowercase").is_err());
        assert!(original.validate("Counter").is_ok());
        assert!(!original.dirty());
    }
    #[test]
    fn preview_uses_unsaved_buffers_and_does_not_touch_generated_files() {
        let root = project();
        let mut d = local(root.path());
        body(
            &mut d,
            "class {{ name }} { final value = '{{variable}}'; }
",
        );
        fs::write(root.path().join("counter.dart"), "existing app code").unwrap();
        let preview = d.validate("Counter").unwrap();
        assert_eq!(
            preview[0],
            (
                PathBuf::from("counter.dart"),
                "class Counter { final value = 'counter'; }
"
                .into()
            )
        );
        assert_eq!(
            fs::read_to_string(root.path().join("counter.dart")).unwrap(),
            "existing app code"
        );
        assert!(
            !fs::read_to_string(d.directory.join("templates/template.dart.tmpl"))
                .unwrap()
                .contains("final value")
        );
    }
    #[test]
    fn edited_pack_save_and_undo_use_original_snapshots() {
        let root = project();
        let mut d = local(root.path());
        let before = d.original.clone();
        body(
            &mut d,
            "class {{name}} { int value = 42; }
",
        );
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("My Template", "Edited Template");
        let applied = d.save_plan("Counter").unwrap().apply().unwrap();
        let pack = engine::load_pack(&d.directory.join("pack.yaml")).unwrap();
        assert_eq!(pack.title, "Edited Template");
        assert!(pack.files[0].content.contains("42"));
        assert!(d.dirty());
        d.mark_saved();
        assert!(!d.dirty());
        assert!(d.save_plan("Counter").is_err());
        applied.undo().unwrap();
        for (p, s) in before {
            assert_eq!(fs::read_to_string(d.directory.join(p)).unwrap(), s);
        }
    }
    #[test]
    fn file_add_remove_and_external_conflicts_are_transactional() {
        let root = project();
        let mut d = local(root.path());
        for path in ["", "../escape", "/absolute", "pack.yaml"] {
            assert!(d.add_file(path).is_err());
        }
        assert!(d.remove_file(Path::new("pack.yaml")).is_err());
        let file = d.add_file("templates/extra.dart.tmpl").unwrap();
        d.files.insert(file.clone(), "// helper".into());
        assert!(d.dirty());
        let plan = d.save_plan("Counter").unwrap();
        fs::write(d.directory.join(&file), "external").unwrap();
        assert!(plan.apply().is_err());
        assert!(d.save_plan("Counter").is_err());
        fs::remove_file(d.directory.join(&file)).unwrap();
        let added = d.save_plan("Counter").unwrap().apply().unwrap();
        assert!(d.directory.join(&file).exists());
        d.mark_saved();
        d.remove_file(&file).unwrap();
        let removed = d.save_plan("Counter").unwrap().apply().unwrap();
        assert!(!d.directory.join(&file).exists());
        removed.undo().unwrap();
        assert_eq!(
            fs::read_to_string(d.directory.join(&file)).unwrap(),
            "// helper"
        );
        added.undo().unwrap();
        assert!(!d.directory.join(&file).exists());
    }
    #[test]
    fn external_source_changes_block_saving_even_when_source_not_edited() {
        let root = project();
        let mut d = local(root.path());
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("My Template", "New Title");
        fs::write(
            d.directory.join("templates/template.dart.tmpl"),
            "// external",
        )
        .unwrap();
        assert!(d.save_plan("Counter").is_err());
        assert_eq!(
            engine::load_pack(&d.directory.join("pack.yaml"))
                .unwrap()
                .title,
            "My Template"
        );
    }
    #[test]
    fn bundled_edit_saves_override_without_mutating_defaults_or_clobbering() {
        let root = project();
        let entry = catalog::bundled_entries()
            .into_iter()
            .find(|e| e.id == "sparrow.state.cubit")
            .unwrap();
        let destination = root.path().join(".sparrow/scaffolds");
        let mut d = Draft::load(&entry, root.path(), &destination).unwrap();
        let original = entry.load().unwrap().files[0].content.clone();
        body(
            &mut d,
            "class {{name}} {}
",
        );
        let applied = d.save_plan("Counter").unwrap().apply().unwrap();
        assert_eq!(entry.load().unwrap().files[0].content, original);
        assert_eq!(
            engine::load_pack(&d.directory.join("pack.yaml"))
                .unwrap()
                .files[0]
                .content,
            "class {{name}} {}
"
        );
        assert!(d.save_plan("Counter").is_err());
        applied.undo().unwrap();
        assert!(!d.directory.exists());
    }
    #[test]
    fn invalid_pack_can_be_repaired_and_disabled_pack_can_be_previewed() {
        let root = project();
        let d = local(root.path());
        let manifest = d.directory.join("pack.yaml");
        let valid = d.files.get(Path::new("pack.yaml")).unwrap().clone();
        fs::write(&manifest, "invalid: [").unwrap();
        let entry = catalog::manager_entries(root.path())
            .into_iter()
            .find(|e| e.id == "team.example")
            .unwrap();
        assert!(entry.problem().is_some());
        let mut repaired = Draft::load(
            &entry,
            root.path(),
            &root.path().join(".sparrow/scaffolds"),
        )
        .unwrap();
        assert!(repaired.validate("Counter").is_err());
        repaired.files.insert(
            "pack.yaml".into(),
            format!(
                "{valid}
disabled: true
"
            ),
        );
        assert!(repaired.validate("Counter").is_ok());
        repaired.save_plan("Counter").unwrap().apply().unwrap();
        assert!(engine::load_pack(&manifest).unwrap().disabled);
    }
    #[test]
    fn all_bundled_draft_previews_match_generation_bytes() {
        let root = project();
        let ctx = engine::context(root.path(), root.path()).unwrap();
        for entry in catalog::bundled_entries() {
            let d = Draft::load(
                &entry,
                root.path(),
                &root.path().join(".sparrow/scaffolds"),
            )
            .unwrap();
            let pack = entry.load().unwrap();
            let plan = match pack.target {
                engine::Target::Package => engine::plan_pack(&ctx, &pack, "Counter"),
                engine::Target::Sibling => {
                    engine::plan_sibling(&ctx, root.path(), &pack, "Counter")
                }
            }
            .unwrap();
            let preview = d.validate("Counter").unwrap();
            for (path, body) in preview {
                assert!(
                    plan.changes
                        .iter()
                        .any(|c| c.path == root.path().join(&path)
                            && c.after.as_deref() == Some(body.as_str())),
                    "{}",
                    entry.id
                );
            }
        }
    }
}
